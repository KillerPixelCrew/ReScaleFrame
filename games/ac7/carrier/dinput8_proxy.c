/* SPDX-License-Identifier: MIT */
/* Research proxy: loads with the game, forwards DirectInput8Create to the real dinput8, and
   dumps the main module once its code section stops looking like ciphertext.

   dinput8 is the carrier because AC7 imports exactly one function from it and nothing else in the
   process does, so the forwarding surface is one export and DXVK is left alone. */

#include <stdlib.h>
#include <rescaleframe/module_dump.h>

#include <rescaleframe/d3d11_host.h>
#include <rescaleframe/d3d11_observer.h>
#include <rescaleframe/overlay_api.h>
#include <rescaleframe/texture_dump.h>
#include <rescaleframe/ac7_carrier.h>
#include <rescaleframe/ac7_motion_capture.h>
#include <rescaleframe/input_dump.h>

#include "dlss_bridge.h"
#include "../../../runtime/orchestrator/src/render_settings_store.h"

#if RSF_HAVE_FRAME_CAPTURE
#include <rescaleframe/frame_capture.h>
#endif

#include <windows.h>

#include <dbghelp.h>

#include <stdint.h>
#include <stdio.h>
#include <wchar.h>
#include <rescaleframe/sha256_file.h>

extern IMAGE_DOS_HEADER __ImageBase;

static HMODULE real_dinput8;
static wchar_t log_path[MAX_PATH * 2];
static wchar_t preference_path[MAX_PATH * 2];

static int executable_sha256(char output[65])
{
    wchar_t path[MAX_PATH];
    unsigned char digest[32];
    unsigned i;
    if (!GetModuleFileNameW(NULL, path, MAX_PATH) ||
        !rsf_common_sha256_file(path, FILE_SHARE_READ, digest)) return 0;
    for (i = 0; i < 32; ++i) snprintf(output + i * 2, 3, "%02x", digest[i]);
    return 1;
}

static void note(const char* format, ...)
{
    if (log_path[0] == L'\0') {
        return;
    }
    FILE* stream = _wfopen(log_path, L"ab");
    if (!stream) {
        return;
    }
    va_list arguments;
    va_start(arguments, format);
    vfprintf(stream, format, arguments);
    va_end(arguments);
    fputc('\n', stream);
    fclose(stream);
}

/* The override that makes the game load this file also makes a load by name come back here, so
   the real library has to be named by absolute path. */
static HMODULE load_real_dinput8(void)
{
    if (real_dinput8) {
        return real_dinput8;
    }
    wchar_t path[MAX_PATH];
    const UINT length = GetSystemDirectoryW(path, MAX_PATH);
    if (length == 0 || length > MAX_PATH - 16) {
        return NULL;
    }
    wcscat(path, L"\\dinput8.dll");
    HMODULE library = LoadLibraryW(path);
    if (library == (HMODULE)&__ImageBase) {
        return NULL; /* Refusing to forward into ourselves. */
    }
    real_dinput8 = library;
    return real_dinput8;
}

typedef HRESULT(WINAPI* direct_input8_create_fn)(HINSTANCE, DWORD, const IID*, void**, void*);

__declspec(dllexport) HRESULT WINAPI DirectInput8Create(HINSTANCE instance, DWORD version,
                                                        const IID* interface_id, void** out,
                                                        void* outer)
{
    HMODULE library = load_real_dinput8();
    if (!library) {
        return E_FAIL;
    }
    const direct_input8_create_fn original = (direct_input8_create_fn)(void*)GetProcAddress(
        library, "DirectInput8Create");
    if (!original || (void*)original == (void*)DirectInput8Create) {
        return E_FAIL;
    }
    return original(instance, version, interface_id, out, outer);
}

/* A file sitting beside this DLL, which is beside the game executable. Written out rather than
   assumed from the working directory, because a game's working directory is not reliably its
   install folder. */
static int beside_this_module(const char* name, char* out, size_t count)
{
    HMODULE self = NULL;
    DWORD length;
    char* separator;

    if (!GetModuleHandleExA(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS |
                                GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                            (LPCSTR)&beside_this_module, &self)) {
        return 0;
    }
    length = GetModuleFileNameA(self, out, (DWORD)count);
    if (length == 0 || length >= count) {
        return 0;
    }
    separator = strrchr(out, '\\');
    if (!separator) {
        return 0;
    }
    separator[1] = '\0';
    if (strlen(out) + strlen(name) >= count) {
        return 0;
    }
    strcat(out, name);
    return 1;
}

/* Settings, from a file beside the proxy rather than from the launch line.

   Every knob here started as an environment variable, which meant a Steam launch option long
   enough to lose a quote in, edited through a dialog, for values that change between runs. The file
   is the same names, one per line, `NAME=value`, with `#` or `;` starting a comment. It is read
   once, at attach.

   An environment variable still wins where it is set, so an existing launch line keeps working and
   a one-off override does not mean editing the file. */
#define RSF_CONFIG_MAX_ENTRIES 64
#define RSF_CONFIG_KEY_BYTES 64
#define RSF_CONFIG_VALUE_BYTES 512

static struct {
    char key[RSF_CONFIG_KEY_BYTES];
    char value[RSF_CONFIG_VALUE_BYTES];
} config_entries[RSF_CONFIG_MAX_ENTRIES];
static int config_count = 0;
static int config_loaded = 0;
static char config_path[MAX_PATH * 2];

static char* trim(char* text)
{
    char* end;
    while (*text == ' ' || *text == '\t') {
        ++text;
    }
    end = text + strlen(text);
    while (end > text && (end[-1] == ' ' || end[-1] == '\t' || end[-1] == '\r' || end[-1] == '\n')) {
        --end;
    }
    *end = '\0';
    return text;
}

static void load_config(void)
{
    FILE* file;
    char line[RSF_CONFIG_KEY_BYTES + RSF_CONFIG_VALUE_BYTES + 4];

    if (config_loaded) {
        return;
    }
    config_loaded = 1;
    if (!beside_this_module("ReScaleFrame.ini", config_path, sizeof(config_path))) {
        return;
    }
    file = fopen(config_path, "r");
    if (!file) {
        return;
    }
    while (fgets(line, (int)sizeof(line), file) && config_count < RSF_CONFIG_MAX_ENTRIES) {
        char* separator;
        char* key;
        char* value;
        char* comment = strpbrk(line, "#;");
        if (comment) {
            *comment = '\0';
        }
        separator = strchr(line, '=');
        if (!separator) {
            continue;
        }
        *separator = '\0';
        key = trim(line);
        value = trim(separator + 1);
        if (!*key || strlen(key) >= RSF_CONFIG_KEY_BYTES ||
            strlen(value) >= RSF_CONFIG_VALUE_BYTES) {
            continue;
        }
        strcpy(config_entries[config_count].key, key);
        strcpy(config_entries[config_count].value, value);
        ++config_count;
    }
    fclose(file);
}

/* A setting's text, or null when nothing sets it. The environment wins over the file. */
static const char* setting(const char* name, char* out, size_t count)
{
    int index;
    if (GetEnvironmentVariableA(name, out, (DWORD)count) != 0) {
        return out;
    }
    load_config();
    for (index = 0; index < config_count; ++index) {
        if (_stricmp(config_entries[index].key, name) == 0) {
            if (strlen(config_entries[index].value) >= count) {
                return NULL;
            }
            strcpy(out, config_entries[index].value);
            return out;
        }
    }
    return NULL;
}

/* A numeric setting.

   Base zero, so the hexadecimal addresses this file documents can be written the way it documents
   them. Absent and zero are different: a setting present and zero is that value, which is what
   makes `RSF_DECODE_MOTION=0` disable decoding and quality zero select Native. Both were review
   findings, and both were consequences of the old parse rejecting anything not strictly positive.
   Text that is not a number at all falls back, because a typo should not read as zero. */
static DWORD read_number(const char* name, DWORD fallback)
{
    char text[64];
    char* end = NULL;
    unsigned long value;
    if (!setting(name, text, sizeof(text)) || !text[0]) {
        return fallback;
    }
    value = strtoul(text, &end, 0);
    if (end == text) {
        return fallback;
    }
    return (DWORD)value;
}

/* The carrier's overlay options, after the host's own: the adapter's startup choices (applied at
   the next start), then the jitter gate, the observer dump and, with capture support, RenderDoc. */
static const wchar_t startup_section[] = L"Startup";
enum {
    OPTION_JITTER = RSF_AC7_CARRIER_START_OPTION_COUNT,
    OPTION_DUMP,
    OPTION_CAPTURE,
    OPTION_LIMIT
};
static rsf_overlay_service_option panel_options[OPTION_LIMIT];
static struct {
    const char* key;
    volatile LONG value;
    uint32_t active_value;
} startup_options[RSF_AC7_CARRIER_START_OPTION_COUNT];

static void load_startup_options(void)
{
    for (uint32_t index = 0; index < RSF_AC7_CARRIER_START_OPTION_COUNT; ++index) {
        rsf_overlay_service_option* option = &panel_options[index];
        uint32_t fallback, maximum;
        const char* key = rsf_ac7_carrier_start_option(index, &option->label_utf8, &fallback, &maximum);
        if (!key) {
            return;
        }
        startup_options[index].key = key;
        option->kind = maximum == 1 ? RSF_OVERLAY_SERVICE_OPTION_TOGGLE : RSF_OVERLAY_SERVICE_OPTION_NUMBER;
        option->maximum = (int32_t)maximum;
        option->step = 1;
        option->divisor = 1;
        option->restart = 1;
        const uint32_t configured = (uint32_t)read_number(key, fallback);
        const uint32_t saved = preference_path[0]
            ? rsf_settings_store_read_integer(preference_path, startup_section, key, configured)
            : configured;
        startup_options[index].active_value = saved <= maximum ? saved : maximum;
        option->value = (int32_t)startup_options[index].active_value;
        InterlockedExchange(&startup_options[index].value, (LONG)startup_options[index].active_value);
    }
}

static void load_carrier_preferences(void)
{
    const DWORD length = GetEnvironmentVariableW(L"LOCALAPPDATA", preference_path, MAX_PATH);
    if (length > 0 && length < MAX_PATH) {
        wcscat(preference_path, L"\\ReScaleFrame");
        if (CreateDirectoryW(preference_path, NULL) || GetLastError() == ERROR_ALREADY_EXISTS) {
            wcscat(preference_path, L"\\AC7.ini");
        } else {
            preference_path[0] = L'\0';
        }
    } else {
        preference_path[0] = L'\0';
    }
    load_startup_options();
}

static int save_startup_option(uint32_t index, int32_t value)
{
    if (!preference_path[0] || index >= RSF_AC7_CARRIER_START_OPTION_COUNT || !startup_options[index].key ||
        value < 0 || value > panel_options[index].maximum) {
        return 0;
    }
    if (rsf_settings_store_write_integer(preference_path, startup_section, startup_options[index].key,
            (uint32_t)value) != RSF_OK) {
        return 0;
    }
    InterlockedExchange(&startup_options[index].value, value);
    return 1;
}

static uint32_t carrier_read_number(const char* name, uint32_t fallback)
{
    for (uint32_t index = 0; index < RSF_AC7_CARRIER_START_OPTION_COUNT; ++index) {
        if (startup_options[index].key && strcmp(startup_options[index].key, name) == 0) {
            return startup_options[index].active_value;
        }
    }
    return (uint32_t)read_number(name, (DWORD)fallback);
}

/* A text setting, with the same precedence. Returns zero when nothing sets it. */
static int read_text(const char* name, char* out, size_t count)
{
    return setting(name, out, count) != NULL && out[0] != '\0';
}

#if RSF_HAVE_FRAME_CAPTURE
/* RenderDoc has to be loaded before the graphics device exists, so this runs on the carrier's own
   attach rather than on the worker thread. It only loads a library and reads two variables.

   The DLL is looked for beside this one before any variable is consulted, because that is where it
   ends up when the proxy is deployed and requiring a path to a file already in the folder is a way
   to press F11 seven times and get nothing. An explicit `RSF_RENDERDOC_DLL` still wins, for a build
   kept somewhere else. */
static void start_capture_support(void)
{
    char library[MAX_PATH];
    char prefix[MAX_PATH];
    rsf_capture_result result;

    /* Off unless asked for, and this is not a convenience default worth having.

       RenderDoc wraps the device, and two things follow that cost a whole session each. NGX refuses
       to create the DLSS feature on a wrapped device, `NGX create feature failed 0xbad00002` once
       per frame forever, which slows the game down until it stops. And RenderDoc makes a device of
       its own, which is how the observer came to hold the wrong one.

       Loading it beside the proxy was meant to save setting a path. It cost more than it saved, so
       the path is still found automatically, but only when capture is actually wanted. */
    const int explicit_library = read_text("RSF_RENDERDOC_DLL", library, sizeof(library));
    if (!explicit_library && read_number("RSF_RENDERDOC", 0) == 0) {
        return;
    }
    if (!explicit_library && !beside_this_module("renderdoc.dll", library, sizeof(library))) {
        note("no renderdoc.dll beside the proxy and RSF_RENDERDOC_DLL is not set, so F11 has "
             "nothing to capture with");
        return;
    }
    if (!read_text("RSF_CAPTURE_PREFIX", prefix, sizeof(prefix))) {
        prefix[0] = '\0';
    }
    result = rsf_capture_initialise(library, prefix[0] ? prefix : NULL);
    /* Announced either way. A capture key that silently does nothing is what this is fixing, and a
       load that failed here is the only place that can say why. */
    note("capture support: %s, result %d", library, (int)result);
}

#endif

static char observe_directory[MAX_PATH];

/* The observer's progress lines go to the same log as everything else. note() opens, writes and
   closes per line, which is what makes the last line before a crash survive it. */
static void observer_note(void* user, const char* message)
{
    (void)user;
    note("%s", message);
}


static void start_observer(void)
{
    if (read_number("RSF_OBSERVE", 1) == 0) {
        note("observer disabled, set RSF_OBSERVE=1 to enable");
        return;
    }

    rsf_observer_options options;
    memset(&options, 0, sizeof(options));
    options.struct_size = sizeof(options);
    options.abi_version = RSF_OBSERVER_ABI_VERSION;
    /* 35 is DXGI_FORMAT_R16G16_UNORM, the format Unreal uses for scene velocity. */
    options.format = read_number("RSF_OBSERVE_FORMAT", 35);
    /* 512 rather than 1024: at half screen percentage the velocity target lands at exactly 1024
       wide, sitting on the old boundary, and a target that is filtered out looks identical to one
       that was never allocated. */
    options.minimum_width = read_number("RSF_OBSERVE_MIN_WIDTH", 512);
    options.capacity = read_number("RSF_OBSERVE_CAPACITY", 8);
    /* A range rather than the stock size: AC7 runs a vendor branch, and the first run showed no
       buffer of the stock 2640 bytes at all. Everything in range is retained per distinct size,
       and every size seen is counted, which is what identifies the right one. */
    options.constant_buffer_min_bytes = read_number("RSF_VIEW_CB_MIN", 1024);
    options.constant_buffer_max_bytes = read_number("RSF_VIEW_CB_MAX", 8192);
    options.log = observer_note;
    {
        char sha256[65] = {0};
        executable_sha256(sha256);
        rsf_input_dump_configure(observe_directory, "AC7", sha256, observer_note, NULL);
    }
    const int motion_capture = carrier_read_number("RSF_MOTION_CAPTURE", 0) != 0;
    rsf_bridge_set_motion_capture(motion_capture);
    if (motion_capture) {
        rsf_ac7_motion_capture_configure(observe_directory, observer_note, NULL);
        options.on_buffer = rsf_ac7_motion_capture_buffer;
    }
    options.decode_motion = read_number("RSF_DECODE_MOTION", 1);
    options.motion_scale = RSF_UNREAL_MOTION_SCALE;
    options.motion_bias = RSF_UNREAL_MOTION_BIAS;
    options.motion_invalid_value = RSF_MOTION_SENTINEL;
    /* Present work arrives through the core host's adapter callback, not the observer. */
    rsf_bridge_set_log(observer_note, NULL);

    /* Input layouts feed the velocity pass and the motion capture; shaders only the capture. */
    options.on_layout = rsf_bridge_layout_hook();
    if (motion_capture) {
        options.on_shader = rsf_bridge_shader_hook();
    }
    const rsf_observer_result result = rsf_observer_install(&options);
    note("observer install result %d (format %lu, min width %lu, view cb %lu..%lu bytes)",
         (int)result, (unsigned long)options.format, (unsigned long)options.minimum_width,
         (unsigned long)options.constant_buffer_min_bytes,
         (unsigned long)options.constant_buffer_max_bytes);
}

/* Revive Unreal's temporal jitter without turning temporal AA on.

   PreVisibilityFrameSetup clears TemporalJitterPixels, then computes a jitter only when the view
   asks for temporal AA:

       cmp  dword ptr [rsi+0x13c0], 2      ; View.AntiAliasingMethod == AAM_TemporalAA
       jne  <skip>                         ; the six bytes replaced below
       test rdi, rdi                       ; && ViewState

   Stepping over that jump lets the jitter run whatever the anti-aliasing setting says, while the
   ViewState check just after it is left alone because a null view state genuinely cannot proceed.
   Unreal then applies the offset to the projection itself, so every matrix derived from it stays
   consistent, which is the reason to do it here rather than editing matrices afterwards.

   The expected bytes are checked before writing. If the game updates and the code moves, this
   refuses rather than corrupting an instruction.

   The patch goes on and off at runtime rather than once at attach. Jitter that nothing resolves is
   visible as a shimmer, and the front end is where it shows: the main menu holds still, so an
   offset that changes every frame has nothing to hide behind. Luma's Unreal path never has this
   problem because it never manufactures jitter, running only where the engine already ran temporal
   AA (`main.cpp:829`) and treating a frame without it as a camera cut (`:1135`). We have to
   manufacture it, because AC7 runs no temporal AA at all and a reconstruction needs the samples.
   What we can copy is the discipline: the jitter exists while something of ours resolves it and at
   no other time. */
static struct {
    int mode;
    int enabled;
    int site_verified;
    DWORD rva;
} jitter_patch;

/* Write the gate open or closed. Idempotent, and announced on every transition: a jitter that
   silently stopped and a jitter that was never on look identical in the image. */
static void set_jitter_enabled(int enabled)
{
    if (jitter_patch.mode == 0 || !jitter_patch.site_verified) {
        return;
    }
    if (jitter_patch.enabled == (enabled != 0)) {
        return;
    }
    uint8_t previous[6] = {0};

    const rsf_dump_result result =
        rsf_ac7_carrier_patch_jitter(rsf_patch_code, jitter_patch.rva, enabled, previous);
    if (result == RSF_DUMP_OK) {
        jitter_patch.enabled = (enabled != 0);
        note("jitter %s at rva 0x%lx", enabled ? "on" : "off", (unsigned long)jitter_patch.rva);
    } else {
        note("jitter %s REFUSED at rva 0x%lx, result %d (bytes were %02x %02x %02x %02x %02x %02x)",
             enabled ? "on" : "off", (unsigned long)jitter_patch.rva, (int)result, previous[0],
             previous[1], previous[2], previous[3], previous[4], previous[5]);
    }
}

/* Called by the bridge when a reconstruction starts resolving frames, and by the panel. A start
   only opens the gate in mode 1; mode 2 has it open already and an explicit click is obeyed in
   either, which is what makes the switch usable as an experiment. */
static void action_set_jitter(unsigned long open)
{
    set_jitter_enabled(open != 0);
}

static unsigned long action_jitter_available(void)
{
    return (unsigned long)(jitter_patch.mode != 0 && jitter_patch.site_verified);
}

/* Record the site and check it, without deciding yet whether the gate is open.

   Mode 2 is the old behaviour, on from attach and never off, kept so a run can compare against
   every result taken before this. Mode 1 follows the reconstruction. Both verify the expected
   bytes here, by opening the gate and closing it again, so a game update is reported at startup
   rather than at the first transition, when whoever is looking is looking at something else. */
static void install_jitter(int32_t mode, uint32_t rva)
{
    jitter_patch.mode = mode;
    if (jitter_patch.mode == 0) {
        return;
    }
    jitter_patch.rva = rva;
    jitter_patch.site_verified = 1;
    jitter_patch.enabled = 0;
    set_jitter_enabled(1);
    if (!jitter_patch.enabled) {
        jitter_patch.site_verified = 0;
        note("jitter gate not found at rva 0x%lx, jitter stays off for this run",
             (unsigned long)jitter_patch.rva);
        return;
    }
    if (jitter_patch.mode == 1) {
        set_jitter_enabled(0);
        note("jitter follows the reconstruction (RSF_ENABLE_JITTER=1); F4 forces it on or off");
    } else {
        note("jitter on for the whole run (RSF_ENABLE_JITTER=2); F4 toggles it");
    }
}

static void report_and_dump(void)
{
    rsf_observer_status status;
    memset(&status, 0, sizeof(status));
    status.struct_size = sizeof(status);
    if (rsf_observer_get_status(&status) != RSF_OBSERVER_OK) {
        note("observer status unavailable");
        return;
    }
    note("observer: %u frames, %u textures created, %u matched, %u view buffers, present %ux%u",
         status.frames_presented, status.textures_created, status.textures_matched,
         status.constant_buffers_matched, status.present_width, status.present_height);

    /* Each press writes its own set. Comparing a menu, a briefing and a mission is how fields
       that the stock layout does not predict get identified: what changes between them says what
       a field is far better than any single snapshot does. */
    static unsigned capture_index = 0;
    const unsigned index = capture_index++;

    char prefix[MAX_PATH * 2];
    snprintf(prefix, sizeof(prefix), "%s\\capture%02u", observe_directory, index);
    /* The work happens inside the next present. Reading a resource from this thread would race
       the game's own rendering, which is what took the process down the first time. */
    const rsf_observer_result requested = rsf_observer_request_dump(prefix, RSF_DUMP_VIEW_ENCODED_MOTION);

    char sizes[MAX_PATH * 2];
    snprintf(sizes, sizeof(sizes), "%s\\capture%02u-buffer-sizes.csv", observe_directory, index);
    rsf_observer_write_buffer_sizes(sizes);

    note("capture %u requested (result %d), %u distinct constant buffer sizes seen", index,
         (int)requested, status.distinct_buffer_sizes);

    /* If DLSS is up, ask for its output too. Comparing it against the game's own inputs from the
       same key press is the only way to tell an evaluate that ran from one that produced a
       picture, and those are different things. */
    if (rsf_bridge_running()) {
        char dlss_prefix[MAX_PATH * 2];
        snprintf(dlss_prefix, sizeof(dlss_prefix), "%s\\capture%02u_dlss", observe_directory,
                 index);
        rsf_bridge_request_dump(dlss_prefix);
    }

    /* Wait briefly for a frame to carry it out, then report what it produced. */
    for (int waited = 0; waited < 100; ++waited) {
        rsf_observer_status after;
        memset(&after, 0, sizeof(after));
        after.struct_size = sizeof(after);
        if (rsf_observer_get_status(&after) == RSF_OBSERVER_OK &&
            after.dumps_completed > status.dumps_completed) {
            note("capture %u finished: %u textures, %u constant bytes", index,
                 after.textures_written, after.constant_bytes_written);
            return;
        }
        Sleep(50);
    }
    note("dump did not complete within five seconds");
}

static int prepare_native_game(void)
{
    wchar_t path[MAX_PATH]; char sha256[65];
    const DWORD length = GetModuleFileNameW((HMODULE)&__ImageBase, path, MAX_PATH);
    wchar_t* separator = length && length < MAX_PATH ? wcsrchr(path, L'\\') : NULL;
    const wchar_t* game_dll = rsf_ac7_carrier_game_dll();
    if (!separator || !executable_sha256(sha256) ||
        (size_t)(separator - path) + 1 + wcslen(game_dll) >= MAX_PATH) return 0;
    wcscpy(separator + 1, game_dll);
    rsf_game_probe probe = {0};
    probe.struct_size = sizeof(probe);
    if (!rsf_ac7_carrier_probe(&probe, sha256)) return 0;
    // Evidence only: the adapter recognizes the executable by name and discovers its sites.
    note("executable SHA-256 %s", sha256);
    return rsf_bridge_prepare_game(path, &probe);
}
static int accept_ac7_window(void* user, void* window)
{
    wchar_t name[64]; DWORD process = 0;
    (void)user;
    GetWindowThreadProcessId((HWND)window, &process);
    if (process != GetCurrentProcessId() || !GetClassNameW((HWND)window, name, 64) ||
        wcscmp(name, rsf_ac7_carrier_window_class()) != 0) return 0;
    // Native preparation discovers and checks its sites in the decrypted image before the cold
    // presentation host creates any D3D12 objects. A refusal keeps the original chain.
    return prepare_native_game();
}
/* The observer dump, on its own thread: it waits for a later frame to carry the request out. */
static DWORD WINAPI dump_thread(LPVOID parameter)
{
    (void)parameter;
    report_and_dump();
    return 0;
}

static void panel_options_read(void* user, int32_t* values, uint32_t count)
{
    (void)user;
    for (uint32_t index = 0; index < count && index < (uint32_t)OPTION_LIMIT; ++index) {
        values[index] = index < RSF_AC7_CARRIER_START_OPTION_COUNT
            ? (int32_t)InterlockedCompareExchange(&startup_options[index].value, 0, 0)
            : index == (uint32_t)OPTION_JITTER ? jitter_patch.enabled : 0;
    }
}

/* Startup choices are saved for the next start; the others act now. */
static int panel_option_changed(void* user, uint32_t index, int32_t value)
{
    (void)user;
    if (index < RSF_AC7_CARRIER_START_OPTION_COUNT) return save_startup_option(index, value);
    switch (index) {
    case OPTION_JITTER:
        if (!action_jitter_available()) return 0;
        action_set_jitter(value != 0);
        return 1;
    case OPTION_DUMP:
        if (value) {
            HANDLE worker = CreateThread(NULL, 0, dump_thread, NULL, 0, NULL);
            if (!worker) return 0;
            CloseHandle(worker);
        }
        return 1;
#if RSF_HAVE_FRAME_CAPTURE
    case OPTION_CAPTURE:
        if (value) {
            const rsf_capture_result result = rsf_capture_trigger(1);
            note("capture triggered from the panel, result %d, captures so far %u", (int)result,
                 rsf_capture_count());
        }
        return 1;
#endif
    default:
        return 0;
    }
}

static void panel_action(uint32_t index, const char* label)
{
    rsf_overlay_service_option* option = &panel_options[index];
    option->label_utf8 = label;
    option->kind = RSF_OVERLAY_SERVICE_OPTION_TOGGLE;
    option->maximum = 1;
    option->step = 1;
    option->divisor = 1;
}

/* The core D3D11 host owns SR, frame generation, pacing, settings and the overlay. AC7 supplies
   its engine facts: Unreal motion encoding, centimetre units, reversed infinite depth, its window
   and native graph, and the overlay options above. */
static void start_host(void)
{
    wchar_t runtime[MAX_PATH];
    char sha256[65] = {0};
    rsf_d3d11_host_setup setup;
    rsf_game_host_services services;
    rsf_d3d11_host_adapter adapter;
    int32_t result;
    const DWORD length = GetModuleFileNameW((HMODULE)&__ImageBase, runtime, MAX_PATH);
    wchar_t* separator = length && length < MAX_PATH ? wcsrchr(runtime, L'\\') : NULL;
    if (!separator || (size_t)(separator - runtime) + 14 >= MAX_PATH) {
        note("core D3D11 host not started: the carrier's folder is unknown");
        return;
    }
    wcscpy(separator + 1, L"ReScaleFrame");
    executable_sha256(sha256);

    memset(&setup, 0, sizeof(setup));
    setup.struct_size = sizeof(setup);
    setup.abi_version = RSF_D3D11_HOST_ABI_VERSION;
    setup.configuration_path = preference_path;
    setup.configuration_section = L"Rendering";
    setup.runtime_directory = runtime;
    setup.identity_utf8 = "AC7";
    setup.executable_sha256 = sha256;
    setup.engine_type = RSF_AC7_ENGINE_TYPE;
    setup.engine_version_utf8 = RSF_AC7_ENGINE_VERSION;
    setup.project_id_utf8 = RSF_AC7_PROJECT_ID;
    /* Unreal units are centimetres and the depth buffer is reversed with an infinite far plane. */
    setup.units_to_meters = 0.01f;
    setup.depth_inverted = 1;
    setup.depth_far_plane = RSF_D3D11_HOST_DEPTH_INFINITE;
    setup.motion.struct_size = sizeof(setup.motion);
    setup.motion.scale_x = RSF_UNREAL_MOTION_SCALE;
    setup.motion.scale_y = RSF_UNREAL_MOTION_SCALE;
    setup.motion.bias_x = RSF_UNREAL_MOTION_BIAS;
    setup.motion.bias_y = RSF_UNREAL_MOTION_BIAS;
    setup.motion.output_scale_x = 1.0f;
    setup.motion.output_scale_y = 1.0f;
    setup.motion.invalid_value = RSF_MOTION_SENTINEL;
    setup.motion.zero_means_unwritten = 1u;
    setup.quality_choices = 0x1fu;
    setup.presentation_bridge = 1u;
    setup.accept_window = accept_ac7_window;
    setup.capabilities = RSF_D3D11_HOST_CAP_FG_INPUTS | RSF_D3D11_HOST_CAP_INPUT_TIMELINE |
        RSF_D3D11_HOST_CAP_NATIVE_GRAPH;

    memset(&services, 0, sizeof(services));
    services.struct_size = sizeof(services);
    services.abi_version = RSF_GAME_ABI_VERSION;
    services.session_id = ((uint64_t)GetCurrentProcessId() << 32) | GetTickCount();
    services.log = observer_note;
    result = rsf_d3d11_host_create(&setup, &services);
    if (result != RSF_OK) {
        note("core D3D11 host refused, result %d; the game keeps its own rendering", (int)result);
        return;
    }
    rsf_bridge_set_host(&services);

    panel_action(OPTION_JITTER, "Temporal jitter gate");
    panel_action(OPTION_DUMP, "Write observer and route dump");
    panel_action(OPTION_CAPTURE, "RenderDoc capture");
    memset(&adapter, 0, sizeof(adapter));
    adapter.struct_size = sizeof(adapter);
    adapter.abi_version = RSF_D3D11_HOST_ABI_VERSION;
#if RSF_HAVE_FRAME_CAPTURE
    adapter.option_count = OPTION_LIMIT;
#else
    adapter.option_count = OPTION_CAPTURE;
#endif
    adapter.options = panel_options;
    adapter.options_read = panel_options_read;
    adapter.option_changed = panel_option_changed;
    adapter.present = rsf_bridge_present;
    result = rsf_d3d11_host_set_adapter(services.user, &adapter);
    if (result != RSF_OK) note("core D3D11 host refused the AC7 options, result %d", (int)result);
    rsf_d3d11_host_set_active(services.user, 1);
    rsf_bridge_set_jitter_control(action_set_jitter);
    note("core D3D11 host started: SR, frame generation, latency and the overlay are the Runtime's");
}
_Static_assert(sizeof(wchar_t) == sizeof(uint16_t), "AC7 carrier UTF-16 ABI");
_Static_assert(sizeof(rsf_ac7_carrier_leaves) == 56, "AC7 carrier ABI");
_Static_assert(offsetof(rsf_ac7_carrier_leaves, read_number) == 8, "AC7 carrier ABI");
_Static_assert(offsetof(rsf_ac7_carrier_leaves, prepare_native_game) == 40, "AC7 carrier ABI");
_Static_assert(offsetof(rsf_ac7_carrier_leaves, install_motion_capture) == 48, "AC7 carrier ABI");
static const rsf_ac7_carrier_leaves carrier_leaves = {
    .struct_size = sizeof(rsf_ac7_carrier_leaves),
    .abi_version = RSF_AC7_CARRIER_ABI_VERSION,
    .read_number = carrier_read_number,
    .patch = rsf_patch_code,
    .log = observer_note,
    .install_jitter = install_jitter,
    .prepare_native_game = prepare_native_game,
    .install_motion_capture = rsf_ac7_motion_capture_install,
};

static int32_t measure_code_entropy(double* entropy)
{
    return rsf_measure_module_code(NULL, entropy);
}

static DWORD WINAPI dump_worker(LPVOID parameter)
{
    (void)parameter;

    char directory[MAX_PATH];
    if (!read_text("RSF_DUMP_DIR", directory, sizeof(directory))) {
        const DWORD length = GetEnvironmentVariableA("LOCALAPPDATA", directory, MAX_PATH);
        if (length == 0 || length + 32 >= MAX_PATH) {
            return 0;
        }
        strcat(directory, "\\ReScaleFrame");
        CreateDirectoryA(directory, NULL);
        strcat(directory, "\\AC7");
    }
    /* Create it rather than failing silently when it is missing. A run of the game is expensive
       enough that losing one to a typo in a path is not acceptable. */
    CreateDirectoryA(directory, NULL);

    MultiByteToWideChar(CP_ACP, 0, directory, -1, log_path, MAX_PATH);
    wcscat(log_path, L"\\rsf-dump.log");

    /* Said up front, because the first Windows run died of it: Steam's overlay hooks Present by
       inline-patching dxgi and by rewriting the swap chain's table, and takes whatever it displaces
       as its original, so a vtable hook on Present and the overlay chase each other. Until the
       observer hooks by detour, the overlay has to be off for this game. */
    if (GetModuleHandleW(L"gameoverlayrenderer64.dll")) {
        note("steam overlay: gameoverlayrenderer64.dll is loaded; the observer detours Present "
             "over its hook and keeps it in the chain, so both run");
    }

    strncpy(observe_directory, directory, MAX_PATH - 1);
    observe_directory[MAX_PATH - 1] = '\0';
    load_carrier_preferences();
    start_observer();
    start_host();

    double entropy = 0.0;
    rsf_measure_module_code(NULL, &entropy);
    note("armed, code entropy %d.%03d", (int)entropy, (int)((entropy - (int)entropy) * 1000));

    rsf_dump_options options;
    memset(&options, 0, sizeof(options));
    options.struct_size = sizeof(options);
    options.abi_version = RSF_MODULE_DUMP_ABI_VERSION;
    options.output_directory_utf8 = directory;
    options.entropy_threshold = 7.0;
    options.require_decrypted = 1;

    rsf_dump_report report;
    memset(&report, 0, sizeof(report));
    report.struct_size = sizeof(report);

    rsf_dump_result result;
    const int dump_module = read_number("RSF_DUMP_MODULE", 0) != 0;
    if (dump_module) {
        result = rsf_dump_when_decrypted(&options, 250, 180000, 2, &report);
    } else {
        result = rsf_ac7_carrier_wait_decrypted(measure_code_entropy, RSF_DUMP_ERROR_STILL_ENCRYPTED);
    }
    if (result != RSF_DUMP_OK) {
        note("startup: executable did not become ready, result %d; the native renderer stays off", (int)result);
        return 0;
    }
    /* Only now: the code was ciphertext until the dump succeeded, so patching earlier would
       write into bytes about to be overwritten. */
    rsf_ac7_carrier_install_decrypted(&carrier_leaves);
    if (!dump_module) {
        note("startup: renderer patches prepared; waiting for a presented frame to start the native renderer");
        return 0;
    }

    note("result %d, entropy %d.%03d, sections %u, imports %u, iat references %u, bytes %llu",
         (int)result, (int)report.code_entropy,
         (int)((report.code_entropy - (int)report.code_entropy) * 1000),
         report.sections_written, report.imports_described, report.iat_references,
         (unsigned long long)report.bytes_written);

    /* Which renderer the game actually chose is only visible once it has initialised one, which
       is long after the code has decrypted. Sample the module list on a schedule instead. */
    char modules_path[MAX_PATH * 2];
    snprintf(modules_path, sizeof(modules_path), "%s\\rsf-modules.log", directory);
    static const DWORD marks[] = {5000, 15000, 30000, 60000, 120000};
    DWORD waited = 0;
    for (size_t index = 0; index < sizeof(marks) / sizeof(marks[0]); ++index) {
        Sleep(marks[index] - waited);
        waited = marks[index];
        char label[64];
        snprintf(label, sizeof(label), "t+%lus", (unsigned long)(waited / 1000));
        rsf_write_module_list(modules_path, label);
    }
    note("module sampling finished");

    /* One automatic report once the game is certainly rendering, so a run that never reaches a
       key press still produces something. F10 repeats it on demand. */
    report_and_dump();
    return 0;
}

/* Crash reporting, because a machine with Windows Error Reporting switched off leaves nothing to
   read when the game dies, and the first Windows run of this proxy did exactly that.

   A vectored handler rather than the top-level filter, so a game that installs its own filter
   cannot silence it, and first in line so nothing consumes the exception before it is described.
   Only the fatal codes are looked at; C++ exceptions and guard page probes pass through untouched.
   It logs the code, the faulting address as module plus offset, the access kind for an access
   violation, and a walk of the faulting thread's stack with whatever symbols dbghelp finds beside
   the modules, then writes a minidump beside the log. Once, because a second fatal exception in a
   dying process is noise. It always returns EXCEPTION_CONTINUE_SEARCH, so whatever the game would
   have done still happens. dbghelp is loaded here rather than linked, since nothing else needs it
   and a process that never crashes never pays for it. */
static volatile LONG crash_reported;

static void describe_address(const void* address, char* out, size_t size)
{
    HMODULE module = NULL;
    wchar_t path[MAX_PATH];
    if (GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS |
                               GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                           (LPCWSTR)address, &module) &&
        GetModuleFileNameW(module, path, MAX_PATH)) {
        const wchar_t* name = wcsrchr(path, L'\\');
        name = name ? name + 1 : path;
        snprintf(out, size, "%p, %ls+0x%llx", address, name,
                 (unsigned long long)((const char*)address - (const char*)module));
    } else {
        snprintf(out, size, "%p, no module", address);
    }
}

typedef BOOL(WINAPI* minidump_write_fn)(HANDLE, DWORD, HANDLE, MINIDUMP_TYPE,
                                        PMINIDUMP_EXCEPTION_INFORMATION,
                                        PMINIDUMP_USER_STREAM_INFORMATION,
                                        PMINIDUMP_CALLBACK_INFORMATION);
typedef BOOL(WINAPI* sym_initialize_fn)(HANDLE, PCSTR, BOOL);
typedef BOOL(WINAPI* sym_from_addr_fn)(HANDLE, DWORD64, PDWORD64, PSYMBOL_INFO);
typedef BOOL(WINAPI* stack_walk_fn)(DWORD, HANDLE, HANDLE, LPSTACKFRAME64, PVOID,
                                    PREAD_PROCESS_MEMORY_ROUTINE64,
                                    PFUNCTION_TABLE_ACCESS_ROUTINE64, PGET_MODULE_BASE_ROUTINE64,
                                    PTRANSLATE_ADDRESS_ROUTINE64);
typedef PVOID(WINAPI* function_table_access_fn)(HANDLE, DWORD64);
typedef DWORD64(WINAPI* module_base_fn)(HANDLE, DWORD64);

/* The description and the dump, on a thread of their own.

   The faulting thread is the wrong place for this work: a stack overflow leaves it one page to
   run on, which the first run of this reporter used up between the first line and the walk, and
   the walk itself has to read that thread's registers from the exception record anyway. So the
   handler hands the exception pointers to a fresh thread and waits for it. The pointers stay valid
   because the faulting thread is blocked in the wait, one frame above the fault. */
struct crash_report {
    EXCEPTION_POINTERS* info;
    DWORD thread;
};

static DWORD WINAPI crash_report_worker(LPVOID parameter)
{
    const struct crash_report* report = (const struct crash_report*)parameter;
    EXCEPTION_POINTERS* info = report->info;
    const EXCEPTION_RECORD* record = info->ExceptionRecord;
    const DWORD code = record->ExceptionCode;
    char where[MAX_PATH + 64];
    HMODULE dbghelp;
    HANDLE process = GetCurrentProcess();
    HANDLE faulting = OpenThread(THREAD_ALL_ACCESS, FALSE, report->thread);

    describe_address(record->ExceptionAddress, where, sizeof(where));
    if (code == EXCEPTION_ACCESS_VIOLATION && record->NumberParameters >= 2) {
        note("crash: access violation %s address %p, code at %s, thread %lu",
             record->ExceptionInformation[0] == 0   ? "reading"
             : record->ExceptionInformation[0] == 1 ? "writing"
                                                    : "executing",
             (void*)record->ExceptionInformation[1], where, report->thread);
    } else {
        note("crash: exception 0x%08lx%s, code at %s, thread %lu", code,
             code == EXCEPTION_STACK_OVERFLOW ? " (stack overflow)" : "", where, report->thread);
    }

    dbghelp = LoadLibraryW(L"dbghelp.dll");
    if (!dbghelp) {
        note("crash: dbghelp.dll did not load, so there is no stack and no dump");
        return 0;
    }
    {
        sym_initialize_fn sym_initialize =
            (sym_initialize_fn)GetProcAddress(dbghelp, "SymInitialize");
        sym_from_addr_fn sym_from_addr = (sym_from_addr_fn)GetProcAddress(dbghelp, "SymFromAddr");
        stack_walk_fn stack_walk = (stack_walk_fn)GetProcAddress(dbghelp, "StackWalk64");
        function_table_access_fn table_access =
            (function_table_access_fn)GetProcAddress(dbghelp, "SymFunctionTableAccess64");
        module_base_fn module_base = (module_base_fn)GetProcAddress(dbghelp, "SymGetModuleBase64");
        if (sym_initialize && sym_from_addr && stack_walk && table_access && module_base &&
            sym_initialize(process, NULL, TRUE)) {
            CONTEXT context = *info->ContextRecord;
            STACKFRAME64 frame;
            int depth;
            memset(&frame, 0, sizeof(frame));
            frame.AddrPC.Offset = context.Rip;
            frame.AddrPC.Mode = AddrModeFlat;
            frame.AddrFrame.Offset = context.Rbp;
            frame.AddrFrame.Mode = AddrModeFlat;
            frame.AddrStack.Offset = context.Rsp;
            frame.AddrStack.Mode = AddrModeFlat;
            /* Sixty-four frames, because a stack overflow is a recursion and the interesting
               part of one is the cycle, which the top few frames only show once. */
            for (depth = 0; depth < 64; ++depth) {
                union {
                    SYMBOL_INFO info;
                    char bytes[sizeof(SYMBOL_INFO) + 256];
                } symbol;
                DWORD64 displacement = 0;
                if (!stack_walk(IMAGE_FILE_MACHINE_AMD64, process,
                                faulting ? faulting : GetCurrentThread(), &frame, &context, NULL,
                                table_access, module_base, NULL) ||
                    frame.AddrPC.Offset == 0) {
                    break;
                }
                memset(&symbol, 0, sizeof(symbol));
                symbol.info.SizeOfStruct = sizeof(SYMBOL_INFO);
                symbol.info.MaxNameLen = 255;
                describe_address((const void*)(uintptr_t)frame.AddrPC.Offset, where,
                                 sizeof(where));
                if (sym_from_addr(process, frame.AddrPC.Offset, &displacement, &symbol.info)) {
                    note("crash:   %2d %s  %s+0x%llx", depth, where, symbol.info.Name,
                         (unsigned long long)displacement);
                } else {
                    note("crash:   %2d %s", depth, where);
                }
            }
        } else {
            note("crash: symbol support unavailable, so there is no stack walk");
        }
    }
    {
        minidump_write_fn write_dump =
            (minidump_write_fn)GetProcAddress(dbghelp, "MiniDumpWriteDump");
        if (write_dump && log_path[0]) {
            wchar_t dump_path[MAX_PATH * 2];
            wchar_t* slash;
            HANDLE file;
            wcscpy(dump_path, log_path);
            slash = wcsrchr(dump_path, L'\\');
            if (slash) {
                slash[1] = L'\0';
            }
            wcscat(dump_path, L"Ace7Game-crash.dmp");
            file = CreateFileW(dump_path, GENERIC_WRITE, 0, NULL, CREATE_ALWAYS,
                               FILE_ATTRIBUTE_NORMAL, NULL);
            if (file != INVALID_HANDLE_VALUE) {
                MINIDUMP_EXCEPTION_INFORMATION exception;
                BOOL written;
                exception.ThreadId = report->thread;
                exception.ExceptionPointers = info;
                exception.ClientPointers = FALSE;
                written = write_dump(process, GetCurrentProcessId(), file,
                                     (MINIDUMP_TYPE)(MiniDumpWithIndirectlyReferencedMemory |
                                                     MiniDumpWithDataSegs |
                                                     MiniDumpWithThreadInfo |
                                                     MiniDumpWithUnloadedModules),
                                     &exception, NULL, NULL);
                CloseHandle(file);
                if (written) {
                    note("crash: minidump written to %ls", dump_path);
                } else {
                    note("crash: minidump failed, error %lu, path %ls", GetLastError(),
                         dump_path);
                }
            }
        }
    }
    if (faulting) {
        CloseHandle(faulting);
    }
    return 0;
}

static LONG CALLBACK on_fatal_exception(EXCEPTION_POINTERS* info)
{
    const DWORD code = info->ExceptionRecord->ExceptionCode;
    struct crash_report report;
    HANDLE worker;

    if (code != EXCEPTION_ACCESS_VIOLATION && code != EXCEPTION_ILLEGAL_INSTRUCTION &&
        code != EXCEPTION_STACK_OVERFLOW && code != EXCEPTION_IN_PAGE_ERROR &&
        code != EXCEPTION_PRIV_INSTRUCTION && code != EXCEPTION_INT_DIVIDE_BY_ZERO) {
        return EXCEPTION_CONTINUE_SEARCH;
    }
    if (InterlockedIncrement(&crash_reported) != 1) {
        return EXCEPTION_CONTINUE_SEARCH;
    }
    report.info = info;
    report.thread = GetCurrentThreadId();
    worker = CreateThread(NULL, 0, crash_report_worker, &report, 0, NULL);
    if (worker) {
        WaitForSingleObject(worker, 30000);
        CloseHandle(worker);
    } else {
        /* No thread can be made while the process is shutting down, which is where two of the
           first day's reports landed. Described on the faulting thread instead, which is fine for
           anything but a stack overflow. */
        note("crash: exception 0x%08lx on thread %lu, no thread could be made, describing inline",
             code, report.thread);
        if (code != EXCEPTION_STACK_OVERFLOW) {
            crash_report_worker(&report);
        }
    }
    return EXCEPTION_CONTINUE_SEARCH;
}

/* Rust (runtime/orchestrator/src/lib.rs). */
void rsf_core_install_panic_record(void);

BOOL WINAPI DllMain(HINSTANCE instance, DWORD reason, LPVOID reserved)
{
    (void)reserved;
    if (reason == DLL_PROCESS_ATTACH) {
        DisableThreadLibraryCalls(instance);
        /* First, before anything that could fault. */
        AddVectoredExceptionHandler(1, on_fatal_exception);
        /* A Rust panic aborts through __fastfail, which the handler above never sees. */
        rsf_core_install_panic_record();
#if RSF_HAVE_FRAME_CAPTURE
        /* Loading a library is allowed here and the timing requirement leaves no alternative:
           RenderDoc must be in before the game creates its device. */
        start_capture_support();
#endif
        /* Everything else happens on workers. DllMain only starts them. */
        HANDLE thread = CreateThread(NULL, 0, dump_worker, NULL, 0, NULL);
        if (thread) {
            CloseHandle(thread);
        }

    }
    return TRUE;
}

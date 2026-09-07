/* SPDX-License-Identifier: GPL-3.0-only */
/* Research proxy: loads with the game, forwards DirectInput8Create to the real dinput8, and
   dumps the main module once its code section stops looking like ciphertext.

   dinput8 is the carrier because AC7 imports exactly one function from it and nothing else in the
   process does, so the forwarding surface is one export and DXVK is left alone. */

#include <rescaleframe/module_dump.h>

#include <rescaleframe/d3d11_observer.h>
#include <rescaleframe/texture_dump.h>

#include "dlss_bridge.h"
#include "overlay_host.h"

#if RSF_HAVE_FRAME_CAPTURE
#include <rescaleframe/frame_capture.h>
#endif

#include <windows.h>

#include <stdint.h>
#include <stdio.h>
#include <wchar.h>

extern IMAGE_DOS_HEADER __ImageBase;

static HMODULE real_dinput8;
static wchar_t log_path[MAX_PATH * 2];

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
    if (read_number("RSF_RENDERDOC", 0) == 0 &&
        !read_text("RSF_RENDERDOC_DLL", library, sizeof(library))) {
        return;
    }
    if (!read_text("RSF_RENDERDOC_DLL", library, sizeof(library)) &&
        !beside_this_module("renderdoc.dll", library, sizeof(library))) {
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

/* Watch for the capture key without touching the game's input. A polled key state cannot disturb
   the message loop or the input ordering the project cares about. */
static DWORD WINAPI capture_worker(LPVOID parameter)
{
    (void)parameter;
    int was_down = 0;
    int running = 1;
    while (running) {
        const int down = (GetAsyncKeyState(VK_F11) & 0x8000) != 0;
        if (down && !was_down) {
            const rsf_capture_result result = rsf_capture_trigger(1);
            note("capture triggered, result %d, captures so far %u", (int)result,
                 rsf_capture_count());
        }
        was_down = down;
        Sleep(50);
    }
    return 0;
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

/* Installed before the module dump rather than after it. The creation hook only sees textures
   made after it is in place, and the target we are after is allocated during engine startup. */
/* Defined below, next to the actions themselves. Declared here because they are registered when
   the observer is installed, which is before anything can call them. */
static void register_overlay_actions(void);

/* Defined next to the other console variable work, and called from the render scale paths above
   it, which run whenever the scale is applied or restored. */
static void set_separate_translucency_scale(void);
static void apply_separate_translucency_patch(void);
static int apply_translucency_depth_patches(void);
/* Whether the engine will build a depth for a layer larger than the scene, which is what a scale
   above 1.0 depends on. Settled at patch time, read whenever the scale is chosen. */
static int translucency_depth_conformed;

static void start_observer(void)
{
    if (read_number("RSF_OBSERVE", 0) == 0) {
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
    /* Unreal's encoding, from Common.ush: In * (0.499 * 0.5) + 32767/65535. Written out as the
       decode a backend needs, which is the reciprocal of that scale and the same bias. The
       sentinel is far outside any real screen space motion and still representable in half. */
    options.decode_motion = read_number("RSF_DECODE_MOTION", 1);
    options.motion_scale = 1.0f / (0.499f * 0.5f);
    options.motion_bias = 32767.0f / 65535.0f;
    options.motion_invalid_value = -1000.0f;
    /* Where the reconstruction gets drawn when it is asked for. Registered at install because the
       observer's options are written once, and harmless until something turns the display on. */
    options.on_present = rsf_bridge_present_hook();
    /* Same reason, and it has to happen before the first present rather than at F8: the overlay
       starts as soon as the game has a device, and the bridge would otherwise have nowhere to
       speak until the backend was started. */
    rsf_bridge_set_log(observer_note, NULL);
    register_overlay_actions();

    /* Naming the game's pipeline objects, so a run can say which draws are the interface. On by
       default because it changes nothing and the answer is what the next milestone needs; the
       hooks are only patched when something asks for them, so turning it off costs the game
       nothing at all. */
    if (read_number("RSF_UI_CLASSIFY", 1) != 0) {
        char forced[512];
        char skipped[512];
        if (!read_text("RSF_UI_SHADER_FORCE", forced, sizeof(forced))) {
            forced[0] = '\0';
        }
        if (!read_text("RSF_UI_SHADER_SKIP", skipped, sizeof(skipped))) {
            skipped[0] = '\0';
        }
        rsf_bridge_name_shaders(forced, skipped);
        rsf_bridge_set_ui_encoding((int)read_number("RSF_UI_ENCODE", 1));
        if (rsf_bridge_identify_ui()) {
            options.on_layout = rsf_bridge_layout_hook();
            options.on_shader = rsf_bridge_shader_hook();
            options.on_texture = rsf_bridge_texture_hook();
            note("ui classification on; converter targets are confirmed by what draws into them, "
                 "not by their shape");
        } else {
            note("ui classification could not start; nothing will be classified this run");
        }
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
    /* jne rel32, the stock gate. */
    static const uint8_t gate[6] = {0x0F, 0x85, 0xDD, 0x03, 0x00, 0x00};
    /* The six byte canonical nop, so the fall-through path is reached. */
    static const uint8_t open[6] = {0x66, 0x0F, 0x1F, 0x44, 0x00, 0x00};
    const uint8_t* write = enabled ? open : gate;
    const uint8_t* expect = enabled ? gate : open;
    uint8_t previous[6] = {0};

    const rsf_dump_result result =
        rsf_patch_code(jitter_patch.rva, write, sizeof(gate), expect, sizeof(gate), previous);
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

static unsigned long action_jitter_open(void)
{
    return (unsigned long)jitter_patch.enabled;
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
static void apply_jitter_patch(void)
{
    jitter_patch.mode = (int)read_number("RSF_ENABLE_JITTER", 0);
    if (jitter_patch.mode == 0) {
        return;
    }
    jitter_patch.rva = read_number("RSF_JITTER_RVA", 0x112b1f3);
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

/* Let translucent geometry into the velocity pass, so a reconstruction has vectors for it.

   Separate translucency carries no motion vectors in stock 4.18, which is why the briefing map's
   relief and the vehicle symbols in mission replay have none. The layer is drawn with scene depth
   bound read only, so it writes no depth either, and depth based camera motion cannot serve it.
   Velocity excludes it by blend mode in FVelocityDrawingPolicyFactory::DrawDynamicMesh:

       call qword ptr [rax+0x198]          ; Material->GetBlendMode()
       cmp  eax, 1                         ; BLEND_Opaque or BLEND_Masked
       ja   <return false>                 ; the six bytes replaced below
       call qword ptr [rax+0x20]           ; GetMaterialDomain, left alone

   Only the blend mode rejection is removed. The material domain check just after it stays, because
   excluding UI domain materials is wanted, and so do the movable test and SupportsVelocity further
   in. A material whose cook produced no usable velocity permutation therefore still refuses rather
   than drawing wrongly, which is what makes this safe to try.

   4.18 compiles velocity shaders for the special engine material, and both draw paths substitute
   that default proxy for a material that writes every pixel, is not two sided and does not move its
   mesh. Sprite like icons should fall into that; two sided sheets should not, and are expected to
   go on refusing. The engine supplies PreviousLocalToWorld itself once a draw reaches the pass,
   which is the whole reason to do this here rather than reconstructing motion outside the game.

   This is the dynamic mesh path. The static equivalent in AddVelocityStaticMesh has not been
   located, so a primitive that renders from the static draw list is unaffected.

   Untested against the game. See docs/research/ue418-hook-map.md. */
static void apply_translucent_velocity_patch(void)
{
    if (read_number("RSF_TRANSLUCENT_VELOCITY", 0) == 0) {
        return;
    }
    const DWORD rva = read_number("RSF_TRANSLUCENT_VELOCITY_RVA", 0x11823de);
    /* ja rel32 */
    const uint8_t expected[6] = {0x0F, 0x87, 0x68, 0x03, 0x00, 0x00};
    /* The six byte canonical nop, so every blend mode falls through to the domain check. */
    const uint8_t replacement[6] = {0x66, 0x0F, 0x1F, 0x44, 0x00, 0x00};
    uint8_t previous[6] = {0};

    const rsf_dump_result result =
        rsf_patch_code(rva, replacement, sizeof(replacement),
                       rva == 0x11823de ? expected : NULL, rva == 0x11823de ? sizeof(expected) : 0,
                       previous);
    if (result == RSF_DUMP_OK) {
        note("translucent velocity gate patched at rva 0x%lx, was %02x %02x %02x %02x %02x %02x. "
             "Translucent draws can now reach the velocity pass; whether any does is a question "
             "for the velocity target, not for this line",
             (unsigned long)rva, previous[0], previous[1], previous[2], previous[3], previous[4],
             previous[5]);
    } else {
        note("translucent velocity gate NOT patched at rva 0x%lx, result %d (expected bytes did "
             "not match?)",
             (unsigned long)rva, (int)result);
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
    const rsf_observer_result requested = rsf_observer_request_dump(prefix, RSF_DUMP_VIEW_VELOCITY);

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
        rsf_bridge_report();
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

static DWORD WINAPI dump_worker(LPVOID parameter)
{
    (void)parameter;

    char directory[MAX_PATH];
    if (!read_text("RSF_DUMP_DIR", directory, sizeof(directory))) {
        note("RSF_DUMP_DIR is not set, nothing to do");
        return 0;
    }
    /* Create it rather than failing silently when it is missing. A run of the game is expensive
       enough that losing one to a typo in a path is not acceptable. */
    CreateDirectoryA(directory, NULL);

    MultiByteToWideChar(CP_ACP, 0, directory, -1, log_path, MAX_PATH);
    wcscat(log_path, L"\\rsf-dump.log");

    strncpy(observe_directory, directory, MAX_PATH - 1);
    observe_directory[MAX_PATH - 1] = '\0';
    start_observer();

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

    const rsf_dump_result result = rsf_dump_when_decrypted(&options, 250, 180000, 2, &report);
    /* Only now: the code was ciphertext until the dump succeeded, so patching earlier would
       write into bytes about to be overwritten. */
    apply_jitter_patch();
    apply_translucent_velocity_patch();
    /* Before the scale patch, because the scale it settles on depends on whether these applied. */
    translucency_depth_conformed = apply_translucency_depth_patches();
    apply_separate_translucency_patch();

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

/* Lengthen the jitter sequence to match the render scale.

   A reconstruction wants each output pixel covered by about the same number of distinct samples
   however many render pixels sit behind it, so the sequence has to grow with the area ratio: eight
   at full scale, thirty two at half. Unreal 4.18 will not do that by itself. It takes the count
   from r.TemporalAASamples and that value does not move with screen percentage, so halving the
   scale without this leaves the sequence a quarter as long as it should be.

   The offset comes from the float variable that was just set, because TConsoleVariableData keeps
   its two thread copies at the same place for every variable of the same element size. Searching
   for the value the way the float path does would not work here: an integer 8 occurs all over the
   object, and replacing every match would corrupt it. */
static void set_jitter_sequence_length(float percentage, uint32_t value_offset)
{
    if (percentage <= 0.0f) {
        return;
    }
    const float ratio = 100.0f / percentage;
    int32_t samples = (int32_t)(8.0f * ratio * ratio + 0.5f);
    if (samples < 8) {
        samples = 8;
    }

    const rsf_dump_result result =
        rsf_console_set_int("r.TemporalAASamples", 8, samples, value_offset,
                            read_number("RSF_CONSOLE_SINGLETON_RVA", 0x3a8b290),
                            read_number("RSF_CONSOLE_FIND_SLOT", 0x90));
    if (result == RSF_DUMP_OK) {
        note("jitter sequence length set to %d at object offset 0x%lx", (int)samples,
             (unsigned long)value_offset);
    } else {
        /* Refusing is the designed outcome when the object does not hold 8 in both slots, which
           covers a different default, a different layout, and the wrong object. */
        note("jitter sequence length NOT set (result %d), offset 0x%lx did not hold 8 twice",
             (int)result, (unsigned long)value_offset);
    }
}

/* Halve the render resolution, the way the engine's own screen percentage does.

   In 4.18 that one cvar is the whole mechanism: it shrinks the scene buffers, makes ViewRect
   differ from UnscaledViewRect so the upscale pass appears, and the jitter formula divides by
   ViewRect so the offset rescales to render resolution by itself. Resizing render targets behind
   the engine's back would instead leave BufferSizeAndInvSize and ScreenPositionScaleBias
   describing a buffer that no longer exists, and every shader does its UV maths with those. */
static void set_screen_percentage(float value)
{
    uint32_t offset = 0;
    const rsf_dump_result result =
        rsf_console_set_float("r.ScreenPercentage", 100.0f, value,
                              read_number("RSF_CONSOLE_SINGLETON_RVA", 0x3a8b290),
                              read_number("RSF_CONSOLE_FIND_SLOT", 0x90), &offset);
    if (result == RSF_DUMP_OK) {
        note("screen percentage set to %d, value found at object offset 0x%lx", (int)value,
             (unsigned long)offset);
        set_jitter_sequence_length(value, offset);
        set_separate_translucency_scale();
        return;
    }

    /* Say what actually went wrong rather than leaving one code to mean several things, and show
       the object, because guessing a layout for a vendor branch is what failed the first time. */
    uint64_t manager = 0, variable = 0;
    float floats[32];
    memset(floats, 0, sizeof(floats));
    const rsf_dump_result probed =
        rsf_console_probe("r.ScreenPercentage", read_number("RSF_CONSOLE_SINGLETON_RVA", 0x3a8b290),
                          read_number("RSF_CONSOLE_FIND_SLOT", 0x90), &manager, &variable, floats,
                          32);
    note("screen percentage not set (set result %d, probe result %d): manager=0x%llx "
         "variable=0x%llx", (int)result, (int)probed, (unsigned long long)manager,
         (unsigned long long)variable);
    if (probed == RSF_DUMP_OK) {
        for (int row = 0; row < 8; ++row) {
            note("  +0x%02x: %12.5f %12.5f %12.5f %12.5f", row * 16, floats[row * 4],
                 floats[row * 4 + 1], floats[row * 4 + 2], floats[row * 4 + 3]);
        }
    }
}

/* Bring DLSS up, on demand rather than at startup.

   The device has to exist first, and at startup it does not. Rather than guessing how long the game
   takes to create one, this is a key press: by the time somebody presses it they are looking at the
   game, so the device is certainly there. It also means a run can reach the menu without loading a
   vendor runtime into the process at all. */
static void start_dlss(void)
{
    char directory[MAX_PATH * 2];
    DWORD width, height;

    /* The render scale first, and on every press rather than only the first.
       Loading a mission re-applies the game's own graphics settings, which puts the screen
       percentage back to 100 and leaves the backend running at the presented size. That is
       antialiasing rather than upscaling, and it is not obvious from the picture: a mission was
       watched that way and read as a successful upscale. Re-applying costs nothing when it is
       already set. */
    set_screen_percentage((float)read_number("RSF_SCREEN_PERCENTAGE", 50));

    if (rsf_bridge_running()) {
        rsf_bridge_report();
        return;
    }
    if (!read_text("RSF_STREAMLINE_BIN", directory, sizeof(directory)) &&
        !beside_this_module("ReScaleFrame\\streamline", directory, sizeof(directory))) {
        note("RSF_STREAMLINE_BIN is not set and no ReScaleFrame\\streamline sits beside the proxy, "
             "so there is nothing to load DLSS from");
        return;
    }

    /* The presented size, from the observer, so this does not have to be told what the game is
       rendering at. Falling back to 1920x1080 would produce a plausible wrong answer, so a missing
       size is a refusal instead. */
    {
        rsf_observer_status status;
        memset(&status, 0, sizeof(status));
        status.struct_size = sizeof(status);
        if (rsf_observer_get_status(&status) != RSF_OBSERVER_OK || status.present_width == 0) {
            note("no presented size yet, so DLSS cannot be told what to output");
            return;
        }
        width = read_number("RSF_DLSS_OUTPUT_WIDTH", status.present_width);
        height = read_number("RSF_DLSS_OUTPUT_HEIGHT", status.present_height);
    }

    note("starting DLSS for %lux%lu output at quality %lu", (unsigned long)width,
         (unsigned long)height, (unsigned long)read_number("RSF_DLSS_QUALITY", 3));
    rsf_bridge_start(directory, width, height, read_number("RSF_DLSS_QUALITY", 3), observer_note,
                     NULL);
    rsf_bridge_report();
}

/* Put the render scale back when the game takes it away.

   Loading a mission re-applies the game's own graphics settings, which sets the screen percentage
   back to 100 and leaves a backend reconstructing from the presented size. That is antialiasing
   rather than upscaling and it does not look like a fault: the picture is clean and sharp precisely
   because nothing was reconstructed from less. A mission was watched that way and read as a
   successful upscale, which is why this is not a key press.

   Nothing here has to detect anything. `rsf_console_set_float` writes only where it finds the value
   it was told to expect, so asking it to replace 100 with our scale does exactly nothing while the
   scale is already ours, and restores it the moment the game puts 100 back. Silent in the ordinary
   case, and it says so on the rare occasion it acts. */
static void keep_render_scale(void)
{
    uint32_t offset = 0;
    const float value = (float)read_number("RSF_SCREEN_PERCENTAGE", 50);
    if (value >= 100.0f) {
        return;
    }
    if (rsf_console_set_float("r.ScreenPercentage", 100.0f, value,
                              read_number("RSF_CONSOLE_SINGLETON_RVA", 0x3a8b290),
                              read_number("RSF_CONSOLE_FIND_SLOT", 0x90), &offset) == RSF_DUMP_OK) {
        note("the game had reset the render scale, put back to %d", (int)value);
        set_jitter_sequence_length(value, offset);
        set_separate_translucency_scale();
    }
}

/* What the overlay panel can ask this file for.

   These are the actions that used to be function keys. They run on the render thread, called from
   the present hook where the panel was drawn, which is the boundary the review finding wants and
   is why the panel can drive them safely when a hotkey worker cannot.

   The scale one remembers what was asked so the panel can show what is in effect. Without it the
   only answer available is the environment default, which stops being true the moment anything
   changes it. */
static unsigned long requested_scale_percent = 0;

static void action_start_backend(void)
{
    start_dlss();
}

/* The separate translucency scale, as the patch below leaves it: a four byte immediate inside the
   instruction stream, which is why it can be changed while the game runs.

   Null until the patch goes in. Nothing else may write it, and a write is a single aligned store,
   so a render thread reading the instruction either sees the old scale or the new one. Both are
   valid floats and neither can be half of the other's bits. */
static volatile uint32_t* translucency_scale_slot;
static float translucency_scale_now;
/* Set by the bridge when this frame's separate translucency layer carried real geometry rather
   than a handful of particles. See action_translucent_geometry. */
static int translucency_layer_heavy;

/* Choose the separate translucency scale for the render scale that is now in effect.

   The scale is a multiplier on the scene buffer, so what it is worth depends entirely on how large
   that buffer is. At a 50% render scale a scale of 1.0 puts the layer at half of native, and 2.0
   puts it at native. That relationship is the whole reason this cannot be a constant: DLSS, XeSS
   and FSR each pick their own render scale per quality level, and a fixed multiplier would mean a
   different translucency resolution for every one of them.

   So the settings are percentages of the presented resolution, and the multiplier is derived:

     RSF_TRANSLUCENCY_TARGET        percent of native, 0 means "match the scene" (scale 1.0)
     RSF_TRANSLUCENCY_TARGET_HEAVY  the same, for a frame whose layer is carrying scene geometry
     RSF_TRANSLUCENCY_SCALE         a direct multiplier in percent, overriding both when nonzero

   The heavy target defaults to 100, which is the briefing case: the relief there is the scene, not
   a decoration over it, and reconstructing it from half of a half was the thing that made it look
   unupscaled. Rendering that layer at native costs a briefing screen nothing worth having. */
static void set_separate_translucency_scale(void)
{
    union {
        float value;
        uint32_t bits;
    } scale;
    unsigned long render_percent;
    unsigned long target;
    const unsigned long override_percent = read_number("RSF_TRANSLUCENCY_SCALE", 0);
    static int ceiling_reported;
    DWORD protection = 0;
    DWORD restored = 0;

    if (!translucency_scale_slot) {
        return;
    }

    render_percent = requested_scale_percent ? requested_scale_percent
                                             : read_number("RSF_SCREEN_PERCENTAGE", 50);
    if (render_percent == 0) {
        render_percent = 100;
    }
    target = translucency_layer_heavy ? read_number("RSF_TRANSLUCENCY_TARGET_HEAVY", 100)
                                      : read_number("RSF_TRANSLUCENCY_TARGET", 0);

    if (override_percent != 0) {
        scale.value = (float)override_percent / 100.0f;
    } else if (target == 0) {
        scale.value = 1.0f;
    } else {
        scale.value = (float)target / (float)render_percent;
    }
    /* The engine clamps its own console value to 100 and this patch is downstream of that clamp,
       so the bound has to be here. */
    if (scale.value < 0.25f) {
        scale.value = 0.25f;
    }
    /* Above the scene's resolution only once the engine can build a depth to match.

       Stock 4.18 borrows the scene's depth for any scale at or above 1.0, which pairs a large
       colour target with a small depth. Game-tested on 7 September: at 2.0 the briefing relief
       disappeared, and the log said why in one line, `last candidate 2048x1152 ... depth view ...
       its texture 1024x576`. `apply_translucency_depth_patches` narrows that borrow to exactly 1.0,
       after which the engine allocates, fills, view-transforms and resolves the layer's own depth
       at whatever size it is.

       So the ceiling follows those patches rather than a setting. If a game update moves them they
       refuse, this stays at the scene's resolution, and the briefing is soft rather than missing. */
    if (scale.value > 1.0f && !translucency_depth_conformed) {
        if (target != 0 && !ceiling_reported) {
            ceiling_reported = 1;
            note("separate translucency asked for %d%% of the scene, but the depth patches did not "
                 "apply, so the engine has no depth to bind at that size. Held at 100%%",
                 (int)(scale.value * 100.0f));
        }
        scale.value = 1.0f;
    }
    if (scale.value > 4.0f) {
        scale.value = 4.0f;
    }

    if (*translucency_scale_slot == scale.bits) {
        return;
    }
    if (!VirtualProtect((LPVOID)translucency_scale_slot, sizeof(uint32_t), PAGE_EXECUTE_READWRITE,
                        &protection)) {
        note("separate translucency scale could not be made writable, left at %d%%",
             (int)(translucency_scale_now * 100.0f));
        return;
    }
    *translucency_scale_slot = scale.bits;
    VirtualProtect((LPVOID)translucency_scale_slot, sizeof(uint32_t), protection, &restored);
    FlushInstructionCache(GetCurrentProcess(), (LPCVOID)translucency_scale_slot, sizeof(uint32_t));
    translucency_scale_now = scale.value;
    note("separate translucency scale now %d%% of the scene, which at a %lu%% render scale is "
         "%d%% of native (%s layer)",
         (int)(scale.value * 100.0f), render_percent,
         (int)(scale.value * (float)render_percent), translucency_layer_heavy ? "heavy" : "light");
}

/* Render separate translucency at the scene's resolution, by taking the halving out.

   The briefing map's relief is separate translucency and arrives at 512x288 while the scene is
   1024x576. Nothing downstream recovers that: by the time anything sees the composite the layer is
   already a doubling of a quarter resolution image. It was never a motion problem.

   `FSceneRenderTargets::SetSeparateTranslucencyBufferSize` computes one scale and uses it three
   times, for the width, the height and the stored scale:

     movss  xmm1, [0.5]                  ; the halving, at 1410be330
     ...
     mulss  xmm0, xmm1                   ; scaled width
     mulss  xmm0, xmm1                   ; scaled height
     movss  [rbx+0x220], xmm1            ; SeparateTranslucencyScale

   The scale arrives at 0.5 two ways: the console variable can say 50, or it can say 100 and the
   automatic downsampling takes over, which is the branch pair just above that load. Setting the
   variable only addresses the second, and this game's value is evidently not the 100 that a write
   guarded on the expected value would accept, because that write never happened.

   So the branch pair and the load are replaced together. Fifteen bytes:

     73 0D                     jnc  +0x0D          ; skip when the scale is not ~1.0
     40 84 FF                  test dil, dil       ; and when nothing asked to downsample
     74 08                     jz   +8
     F3 0F 10 0D 58 61 4B 01   movss xmm1, [0.5]

   The first version of this loaded the 1.0 that sits four bytes after the 0.5 in the same pool,
   which fixed the briefing relief and the cannon tracers at once but fixed the scale at exactly the
   scene's resolution. That is not enough, because the right scale depends on the render scale and
   the render scale moves with the quality level. The pool has no 1.5 and no 2.0 next to the 0.5,
   and hunting one elsewhere would only trade one constant for another.

   Carrying the value in the instruction instead answers both. The disassembly settles the one
   question that needs settling, which is whether a register is free:

     1410be338  movd  xmm0, dword ptr [rbx+0x208]     ; does not read eax
     1410be346  mov   rax, qword ptr [rbx+0x208]      ; overwrites rax outright

   Every path out of the patched window reaches those, so eax is dead across it and can carry the
   float. Fifteen bytes become:

     66 0F 1F 44 00 00         nop  word ptr [rax+rax*1]
     B8 xx xx xx xx            mov  eax, <scale bits>
     66 0F 6E C8               movd xmm1, eax

   The six byte nop leads so the immediate lands at rva+7, which is 0x10be330 and four byte
   aligned. That alignment is the point: `set_separate_translucency_scale` above changes the scale
   by storing one aligned word into it while the game runs, and an aligned store cannot be seen
   half done. Everything after still reads xmm1, so the width, the height and the stored
   SeparateTranslucencyScale all follow it.

   The expected bytes are checked before writing. If the game updates and this moves, it refuses
   rather than corrupting an instruction. An overridden RVA that would leave the immediate
   unaligned is patched but not registered, so it keeps its startup scale and never changes. */
static void apply_separate_translucency_patch(void)
{
    if (read_number("RSF_FULL_TRANSLUCENCY", 1) == 0) {
        return;
    }
    const DWORD rva = read_number("RSF_FULL_TRANSLUCENCY_RVA", 0x10be329);
    const uint8_t expected[15] = {0x73, 0x0D, 0x40, 0x84, 0xFF, 0x74, 0x08, 0xF3,
                                  0x0F, 0x10, 0x0D, 0x58, 0x61, 0x4B, 0x01};
    const uint8_t replacement[15] = {0x66, 0x0F, 0x1F, 0x44, 0x00, 0x00, /* nop word */
                                     0xB8, 0x00, 0x00, 0x80, 0x3F,       /* mov eax, 1.0f */
                                     0x66, 0x0F, 0x6E, 0xC8};            /* movd xmm1, eax */
    uint8_t previous[15] = {0};
    unsigned char* base;
    unsigned char* immediate;

    const rsf_dump_result result =
        rsf_patch_code(rva, replacement, sizeof(replacement),
                       rva == 0x10be329 ? expected : NULL, rva == 0x10be329 ? sizeof(expected) : 0,
                       previous);
    if (result != RSF_DUMP_OK) {
        note("separate translucency halving NOT removed at rva 0x%lx, result %d (expected bytes "
             "did not match?)",
             (unsigned long)rva, (int)result);
        return;
    }

    base = (unsigned char*)GetModuleHandleW(NULL);
    immediate = base ? base + rva + 7 : NULL;
    if (!immediate || ((uintptr_t)immediate & 3u) != 0) {
        note("separate translucency halving removed at rva 0x%lx, but its scale sits at an "
             "unaligned address and stays at 100%% of the scene for this run",
             (unsigned long)rva);
        return;
    }
    translucency_scale_slot = (volatile uint32_t*)(void*)immediate;
    translucency_scale_now = 1.0f;
    note("separate translucency halving removed at rva 0x%lx, scale carried at 0x%p and "
         "adjustable while the game runs",
         (unsigned long)rva, (void*)immediate);
    set_separate_translucency_scale();
}

/* Let the engine build a depth for a separate translucency layer larger than the scene.

   4.18 decides four times whether the layer has its own depth or borrows the scene's, and every one
   of them asks `Scale < 1.f`. Below 1.0 that is the downsampling case and the engine allocates a
   depth at the layer's size, fills it, builds a view uniform buffer for the scaled rect and
   resolves it. At exactly 1.0 the layer is the scene's size and the scene's depth fits. Above 1.0
   all four take the borrow branch, which pairs a large colour target with a small depth. D3D11 does
   not allow that pair, and game-testing it on 7 September made the briefing relief disappear
   entirely while its HUD stayed.

   Asking `Scale == 1.f` instead is the whole fix, because the borrow is correct only at exactly
   1.0. Everything else the engine already does correctly at any scale: `DownsampleDepthSurface`
   takes the factor as a parameter and sets its viewport and rectangle from it, so at 2.0 it simply
   upsamples, and `SetupDownsampledTranslucencyViewUniformBuffer` rebuilds the view from
   `ScaledSize` and `ViewRect * scale`. Nothing here adds a shader, a hook or a resource.

   The four sites, against 4.18.3 source:

     TranslucentRendering.cpp:1258   0x1168f6f  76 0E  jbe   skips the depth allocation and fill
     SceneRenderTargets.cpp:1373     0x1097a0c  76 17  jbe   binds scene depth instead
     SceneRenderTargets.cpp:1400     0x109d03a  cmova        picks scene depth to resolve
     SceneRenderTargets.cpp:1405     0x109d061  76 17  jbe   the same, on the other branch

   `jbe` becomes `je` and `cmova` becomes `cmovne`, one byte each. With `comiss 1.0, scale` the two
   are the same instruction for every scale the engine can produce on its own: at 1.0 both act on
   ZF, and below 1.0 neither fires. They differ only above 1.0, which is a state only our own scale
   patch can reach. So this changes nothing about stock rendering, and it is what the scale above
   1.0 is allowed to depend on.

   All four or none. A partial application would leave the engine allocating a depth it does not
   bind, so a failure here keeps the scale clamped to the scene's resolution. */
static int apply_translucency_depth_patches(void)
{
    static const struct {
        DWORD rva;
        uint8_t count;
        uint8_t expected[4];
        uint8_t replacement[4];
        const char* what;
    } sites[] = {
        {0x1168f6f, 2, {0x76, 0x0E}, {0x74, 0x0E}, "the depth allocation and fill"},
        {0x1097a0c, 2, {0x76, 0x17}, {0x74, 0x17}, "the depth bind"},
        {0x109d03a, 4, {0x44, 0x0F, 0x47, 0xF9}, {0x44, 0x0F, 0x45, 0xF9}, "the snapshot resolve"},
        {0x109d061, 2, {0x76, 0x17}, {0x74, 0x17}, "the resolve"},
    };
    uint8_t previous[4] = {0};
    unsigned int i;

    for (i = 0; i < sizeof(sites) / sizeof(sites[0]); ++i) {
        const rsf_dump_result result =
            rsf_patch_code(sites[i].rva, sites[i].replacement, sites[i].count, sites[i].expected,
                           sites[i].count, previous);
        if (result != RSF_DUMP_OK) {
            note("translucency depth: %s at rva 0x%lx did NOT match, result %d. The layer stays at "
                 "the scene's resolution",
                 sites[i].what, (unsigned long)sites[i].rva, (int)result);
            return 0;
        }
        note("translucency depth: %s at rva 0x%lx now asks for exactly 1.0 rather than less",
             sites[i].what, (unsigned long)sites[i].rva);
    }
    return 1;
}

static void action_set_render_scale(unsigned long percent)
{
    if (percent == 0 || percent > 100) {
        return;
    }
    requested_scale_percent = percent;
    set_screen_percentage((float)percent);
    /* Again here rather than only inside set_screen_percentage, which reaches it only when the
       console write actually happened. A scale that is already ours writes nothing, and the
       translucency multiplier still has to be recomputed against it. */
    set_separate_translucency_scale();
}

static unsigned long action_render_scale_percent(void)
{
    return requested_scale_percent;
}

static void action_trigger_dump(void)
{
    report_and_dump();
}

static void action_trigger_capture(void)
{
#if RSF_HAVE_FRAME_CAPTURE
    const rsf_capture_result result = rsf_capture_trigger(1);
    note("capture triggered from the panel, result %d, captures so far %u", (int)result,
         rsf_capture_count());
#else
    note("this build has no frame capture support");
#endif
}

static unsigned long action_capture_count(void)
{
#if RSF_HAVE_FRAME_CAPTURE
    return rsf_capture_count();
#else
    return 0;
#endif
}

/* How much geometry went into the separate translucency layer this frame, from the bridge.

   The briefing relief and a burst of cannon tracers are the same kind of surface to the engine and
   are told apart by how much of it there is: the captured briefing layer is 59 draws and 540,030
   indices, and gameplay effects are orders of magnitude below that. So the rule is a threshold on
   the count rather than any attempt to recognise a screen, and the count is logged the first few
   times it crosses so the threshold can be set from what the game actually draws.

   The scale it selects lands on the next frame's buffer allocation, not this one. A briefing lasts
   thousands of frames and a tracer burst lasts tens, so a frame of latency is invisible in the one
   case and is the reason the other never triggers a resize storm. */
static void action_translucent_geometry(unsigned long indices)
{
    static unsigned long crossings;
    const unsigned long threshold = read_number("RSF_TRANSLUCENCY_HEAVY_INDICES", 100000);
    const int heavy = threshold != 0 && indices >= threshold;
    if (heavy == translucency_layer_heavy) {
        return;
    }
    translucency_layer_heavy = heavy;
    if (crossings < 8) {
        ++crossings;
        note("separate translucency layer became %s: %lu indices this frame against a threshold "
             "of %lu",
             heavy ? "heavy" : "light", indices, threshold);
    }
    set_separate_translucency_scale();
}

/* What reinsertion does when a promoted target meets the game's render resolution depth.

   0 drops the depth, so the pass draws without its depth test. 1 forwards both, which is an invalid
   pair and draws nothing. 2 leaves the target alone, so the pass draws into a texture nothing reads.
   All three are wrong; dropping is the only one that keeps the pixels, so it is the default. A
   setting rather than a constant so all three can be compared in one run. */
static unsigned long action_reinsert_depth_policy(void)
{
    return read_number("RSF_REINSERT_DEPTH", 0);
}

static void register_overlay_actions(void)
{
    rsf_bridge_actions actions;
    memset(&actions, 0, sizeof(actions));
    actions.start_backend = action_start_backend;
    actions.translucent_geometry = action_translucent_geometry;
    actions.reinsert_depth_policy = action_reinsert_depth_policy;
    actions.set_render_scale = action_set_render_scale;
    actions.trigger_dump = action_trigger_dump;
    actions.trigger_capture = action_trigger_capture;
    actions.capture_count = action_capture_count;
    actions.render_scale_percent = action_render_scale_percent;
    actions.set_jitter = action_set_jitter;
    actions.jitter_open = action_jitter_open;
    actions.jitter_available = action_jitter_available;
    rsf_bridge_set_actions(&actions);
}

static DWORD WINAPI observe_worker(LPVOID parameter)
{
    (void)parameter;
    int was_down = 0;
    int scale_down = 0;
    int dlss_down = 0;
    int show_down = 0;
    int panel_down = 0;
    int reinsert_down = 0;
    int jitter_down = 0;
    int extract_down = 0;
    int ticks = 0;
    int running = 1;
    while (running) {
        const int dlss = (GetAsyncKeyState(VK_F8) & 0x8000) != 0;
        if (dlss && !dlss_down) {
            start_dlss();
        }
        dlss_down = dlss;

        {
            /* The overlay's own toggle is the window procedure's, which is the right place for it:
               it can swallow the key so the game does not also act on it. It is not always
               reached, though. A run turned up where the panel never opened and no toggle ever
               arrived, so the key never became a message for the window the swap chain named.

               Polling here as well costs nothing and does not depend on which window has focus.
               Both paths end in the same set_visible, and the input module ignores a transition to
               the state it is already in, so pressing F5 once cannot toggle twice. */
            const int panel = (GetAsyncKeyState(VK_F5) & 0x8000) != 0;
            if (panel && !panel_down) {
                rsf_overlay_host_toggle();
            }
            panel_down = panel;
        }

        {
            const int show = (GetAsyncKeyState(VK_F7) & 0x8000) != 0;
            if (show && !show_down) {
                rsf_bridge_toggle_display();
            }
            show_down = show;
        }

        {
            /* The real path, as against F7's debug view: the reconstruction goes into the game's
               own frame before the tonemap, so the grade and the interface are the game's. It needs
               the frame's tail identified first, which takes a few frames after F8. */
            const int reinsert = (GetAsyncKeyState(VK_F6) & 0x8000) != 0;
            if (reinsert && !reinsert_down) {
                rsf_bridge_toggle_reinsert();
            }
            reinsert_down = reinsert;
        }

        /* Report on a timer as well as on the key, because the report a key press produces is
           taken the instant the tap is installed and therefore says nothing. Reading it as a result
           cost a whole run. The report stays quiet while the numbers do not move, so a session that
           reaches a steady state stops writing. */
        if (++ticks >= 20 && rsf_bridge_running()) {
            ticks = 0;
            /* Every second, because a mission load puts the game's own screen percentage back and
               a backend then reconstructs from the presented size without anything looking wrong.
               This does nothing at all while the scale is already ours. */
            keep_render_scale();
            rsf_bridge_report();
        }

        const int down = (GetAsyncKeyState(VK_F10) & 0x8000) != 0;
        if (down && !was_down && observe_directory[0]) {
            report_and_dump();
        }
        was_down = down;

        const int scale = (GetAsyncKeyState(VK_F9) & 0x8000) != 0;
        if (scale && !scale_down) {
            set_screen_percentage((float)read_number("RSF_SCREEN_PERCENTAGE", 50));
        }
        scale_down = scale;

        {
            /* Extraction, on a key rather than only in the settings, because the thing it changes
               is the picture and the comparison that matters is before against after on the same
               screen. It needs the presented size, which is only known once the game has a swap
               chain, so it cannot simply be applied at attach. */
            const int extract = (GetAsyncKeyState(VK_F3) & 0x8000) != 0;
            if (extract && !extract_down) {
                rsf_observer_status status;
                memset(&status, 0, sizeof(status));
                status.struct_size = sizeof(status);
                if (rsf_observer_get_status(&status) == RSF_OBSERVER_OK && status.present_width) {
                    rsf_bridge_extract_ui(status.present_width, status.present_height);
                } else {
                    note("ui extract: the presented size is not known yet");
                }
            }
            extract_down = extract;
        }

        {
            /* Flip the jitter while looking at the screen that shows it. A shimmering front end
               has two possible causes and they need different fixes: an offset nothing resolves,
               or a resolve that fails on elements with no motion vectors. Holding still on the
               main menu and pressing this separates them in one press, which no counter can. */
            const int jitter = (GetAsyncKeyState(VK_F4) & 0x8000) != 0;
            if (jitter && !jitter_down) {
                set_jitter_enabled(!jitter_patch.enabled);
            }
            jitter_down = jitter;
        }
        Sleep(50);
    }
    return 0;
}

BOOL WINAPI DllMain(HINSTANCE instance, DWORD reason, LPVOID reserved)
{
    (void)reserved;
    if (reason == DLL_PROCESS_ATTACH) {
        DisableThreadLibraryCalls(instance);
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
#if RSF_HAVE_FRAME_CAPTURE
        thread = CreateThread(NULL, 0, capture_worker, NULL, 0, NULL);
        if (thread) {
            CloseHandle(thread);
        }
#endif
        thread = CreateThread(NULL, 0, observe_worker, NULL, 0, NULL);
        if (thread) {
            CloseHandle(thread);
        }
    }
    return TRUE;
}

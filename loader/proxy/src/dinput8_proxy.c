/* SPDX-License-Identifier: GPL-3.0-only */
/* AC7 carrier: forwards DirectInput8Create, installs observation/presentation hooks,
   waits for decrypted executable code, and prepares reconstruction on the render thread.
   Worker readiness is published through startup_ready; device/context work runs at Present.
   Executable dumps and RenderDoc are optional diagnostics. Patch evidence and build fingerprints
   are recorded in docs/research/ue418-hook-map.md. */

#include <stdlib.h>
#include <rescaleframe/module_dump.h>

#include <rescaleframe/d3d11_observer.h>
#include <rescaleframe/overlay_input.h>
#include <rescaleframe/texture_dump.h>
#include <rescaleframe/ac7_scene_color.h>
#include <rescaleframe/ac7_motion_capture.h>
#include <rescaleframe/native_fg.h>
#include <rescaleframe/dlss_pipeline.h>

#include "dlss_bridge.h"
#include "overlay_host.h"
#include "preferences.h"
#include <rescaleframe/fg_choice.h>

#if RSF_HAVE_FRAME_CAPTURE
#include <rescaleframe/frame_capture.h>
#endif

#include <windows.h>

#include <dbghelp.h>

#include <stdint.h>
#include <stdio.h>
#include <wchar.h>
#include <bcrypt.h>

extern IMAGE_DOS_HEADER __ImageBase;

static HMODULE real_dinput8;
static wchar_t log_path[MAX_PATH * 2];
static volatile LONG startup_ready;
static wchar_t preference_path[MAX_PATH * 2];
static uint32_t preferred_enabled = 1;
static uint32_t preferred_quality = 3;

/* Hash the on-disk executable into 64 lowercase hexadecimal digits plus terminator.
   Returns zero on file/CNG failure; all local handles are released before returning. */
static int executable_sha256(char output[65])
{
    wchar_t path[MAX_PATH];
    BCRYPT_ALG_HANDLE algorithm = NULL;
    BCRYPT_HASH_HANDLE hash = NULL;
    HANDLE file = INVALID_HANDLE_VALUE;
    unsigned char digest[32], buffer[65536];
    DWORD read = 0;
    int ok = 0;
    unsigned i;
    if (!GetModuleFileNameW(NULL, path, MAX_PATH)) return 0;
    file = CreateFileW(path, GENERIC_READ, FILE_SHARE_READ, NULL, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, NULL);
    if (file == INVALID_HANDLE_VALUE || BCryptOpenAlgorithmProvider(&algorithm, BCRYPT_SHA256_ALGORITHM, NULL, 0) < 0 ||
        BCryptCreateHash(algorithm, &hash, NULL, 0, NULL, 0, 0) < 0) goto done;
    for (;;) {
        if (!ReadFile(file, buffer, sizeof(buffer), &read, NULL)) goto done;
        if (!read) break;
        if (BCryptHashData(hash, buffer, read, 0) < 0) goto done;
    }
    if (read || BCryptFinishHash(hash, digest, sizeof(digest), 0) < 0) goto done;
    for (i = 0; i < 32; ++i) snprintf(output + i * 2, 3, "%02x", digest[i]);
    ok = 1;
done:
    if (hash) BCryptDestroyHash(hash);
    if (algorithm) BCryptCloseAlgorithmProvider(algorithm, 0);
    if (file != INVALID_HANDLE_VALUE) CloseHandle(file);
    return ok;
}

/* Best-effort append-only logging; missing paths or write failures are intentionally silent. */
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

/* Preserve the system DirectInput ABI and return its HRESULT. Failed or recursive
   resolution returns E_FAIL without running renderer startup. */
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

/* Resolve a path relative to this DLL, independent of the process working directory.
   Writes a terminated path into out and returns zero on lookup or capacity failure. */
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

/* Installation settings use NAME=value lines in ReScaleFrame.ini beside the carrier.
   The first lookup caches at most 64 entries; # and ; begin comments anywhere on a line.
   Environment values take precedence; file keys are compared case-insensitively and the first
   duplicate wins. Saved enable/quality preferences are applied after these defaults. */
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

/* Load installation defaults once. Ignore malformed/oversized lines; cap retained entries. */
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

/* Parse an unsigned setting with strtoul base zero, preserving explicit zero.
   Empty text or a missing numeric prefix uses fallback. Trailing text and range overflow are
   not separately rejected; this is the legacy diagnostic parser, not a strict INI validator. */
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
/* Opt-in capture initialization at process attach, before any graphics device is created.
   RSF_RENDERDOC_DLL overrides renderdoc.dll beside the carrier; RSF_CAPTURE_PREFIX is optional. */
static void start_capture_support(void)
{
    char library[MAX_PATH];
    char prefix[MAX_PATH];
    rsf_capture_result result;

    /* Keep capture opt-in: the observed RenderDoc wrapper caused NGX feature creation
       refusal and could make the observer select a helper device. See loader/README.md. */
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

    note("capture support: %s, result %d", library, (int)result);
}

#endif

static char observe_directory[MAX_PATH];

/* Adapt module logs to the carrier sink. Each note opens and closes the file so completed
   lines survive most crashes. */
static void observer_note(void* user, const char* message)
{
    (void)user;
    note("%s", message);
}



static void register_overlay_actions(void);


static void set_separate_translucency_scale(void);
static void apply_separate_translucency_patch(void);
static int apply_translucency_depth_patches(void);
/* Whether the engine will build a depth for a layer larger than the scene, which is what a scale
   above 1.0 depends on. Settled at patch time, read whenever the scale is chosen. */
static int translucency_depth_conformed;

/* Install creation and Present hooks before engine resources appear. Register UI rules,
   capture configuration and carrier actions before callbacks can execute. */
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
    /* Include velocity targets at reduced render resolution. */
    options.minimum_width = read_number("RSF_OBSERVE_MIN_WIDTH", 512);
    options.capacity = read_number("RSF_OBSERVE_CAPACITY", 8);
    /* AC7 uses a vendor view-buffer layout; retain/count sizes across this diagnostic range. */
    options.constant_buffer_min_bytes = read_number("RSF_VIEW_CB_MIN", 1024);
    options.constant_buffer_max_bytes = read_number("RSF_VIEW_CB_MAX", 8192);
    options.log = observer_note;
    if (read_number("RSF_MOTION_CAPTURE", 0) != 0) {
        rsf_ac7_motion_capture_configure(observe_directory, observer_note, NULL);
        options.on_buffer = rsf_ac7_motion_capture_buffer;
    }
    /* Unreal's encoding, from Common.ush: In * (0.499 * 0.5) + 32767/65535. Written out as the
       decode a backend needs, which is the reciprocal of that scale and the same bias. The
       sentinel is far outside any real screen space motion and still representable in half. */
    options.decode_motion = read_number("RSF_DECODE_MOTION", 1);
    options.motion_scale = 1.0f / (0.499f * 0.5f);
    options.motion_bias = 32767.0f / 65535.0f;
    options.motion_invalid_value = -1000.0f;
    /* Register callbacks before the first Present; observer options are immutable after install. */
    options.on_present = rsf_bridge_present_hook();
    options.on_present_event = rsf_bridge_present_event_hook();
    /* Publish logging/actions before the first overlay frame. */
    rsf_bridge_set_log(observer_note, NULL);
    {
        char prefix[MAX_PATH * 2];
        if (read_text("RSF_BRIEFING_CAPTURE_PREFIX", prefix, sizeof(prefix))) {
            rsf_bridge_set_briefing_capture(prefix);
        }
    }
    register_overlay_actions();

    /* Classify object creation without diverting draws. Hooks must exist before creation
       to associate borrowed object addresses with layouts and shader hashes. */
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

    if (read_number("RSF_MOTION_CAPTURE", 0) != 0) {
        options.on_shader = rsf_bridge_shader_hook();
    }
    const rsf_observer_result result = rsf_observer_install(&options);
    note("observer install result %d (format %lu, min width %lu, view cb %lu..%lu bytes)",
         (int)result, (unsigned long)options.format, (unsigned long)options.minimum_width,
         (unsigned long)options.constant_buffer_min_bytes,
         (unsigned long)options.constant_buffer_max_bytes);
}

/* Verified AA-gate patch state for PreVisibilityFrameSetup. The patch preserves ViewState
   validation and applies engine-generated jitter through the normal projection path. Mode 1
   follows reconstruction; mode 2 opens at startup. A main-view stub avoids jittering secondary
   views that reconstruction never consumes. See docs/research/ue418-hook-map.md. */
static struct {
    int mode;
    int enabled;
    int site_verified;
    DWORD rva;
    /* The main-view stub, when the gate opens through it rather than for every view. */
    unsigned char* stub;
    volatile LONG* stub_width;
    volatile LONG* stub_height;
    /* The main view the stub last let through, kept for inspection in a dump; how many views it
       let through and turned away; and the width or height that last failed the test. */
    void* volatile* stub_view;
    volatile LONG* stub_allowed;
    volatile LONG* stub_denied;
    volatile LONG* stub_last_denied;
} jitter_patch;

/* Allocate a process-lifetime executable stub within rel32 reach of the verified gate.
   rsi names the view; ViewRect is at +0x70..+0x7c. Stock temporal-AA views continue normally;
   other views pass only when their rectangle matches the last observed main-view render size.
   A zero width permits all views during discovery. The stub preserves rax and branches back to
   gate+6 or the stock skip target; its counters and dimensions occupy the trailing data area.
   Restricted to the researched RVA because its register and branch assumptions are site-specific. */
static int build_jitter_stub(DWORD rva)
{
    unsigned char* base = (unsigned char*)GetModuleHandleW(NULL);
    unsigned char* gate;
    unsigned char* continue_at;
    unsigned char* skip_at;
    unsigned char* stub = NULL;
    uintptr_t candidate;
    int32_t displacement;
    static const unsigned char body[82] = {
        0x0F, 0x84, 0, 0, 0, 0,             /* 0  je continue            */
        0x50,                               /* 6  push rax               */
        0x8B, 0x05, 75, 0, 0, 0,            /* 7  mov eax, [width]       */
        0x85, 0xC0,                         /* 13 test eax, eax          */
        0x74, 28,                           /* 15 je allow               */
        0x8B, 0x46, 0x78,                   /* 17 mov eax, [rsi+78]      */
        0x2B, 0x46, 0x70,                   /* 20 sub eax, [rsi+70]      */
        0x3B, 0x05, 59, 0, 0, 0,            /* 23 cmp eax, [width]       */
        0x75, 33,                           /* 29 jne deny               */
        0x8B, 0x46, 0x7C,                   /* 31 mov eax, [rsi+7c]      */
        0x2B, 0x46, 0x74,                   /* 34 sub eax, [rsi+74]      */
        0x3B, 0x05, 49, 0, 0, 0,            /* 37 cmp eax, [height]      */
        0x75, 19,                           /* 43 jne deny               */
        0x48, 0x89, 0x35, 44, 0, 0, 0,      /* 45 allow: mov [view], rsi */
        0xFF, 0x05, 46, 0, 0, 0,            /* 52 inc dword [allowed]    */
        0x58,                               /* 58 pop rax                */
        0xE9, 0, 0, 0, 0,                   /* 59 jmp continue           */
        0x89, 0x05, 42, 0, 0, 0,            /* 64 deny: mov [last], eax  */
        0xFF, 0x05, 32, 0, 0, 0,            /* 70 inc dword [denied]     */
        0x58,                               /* 76 pop rax                */
        0xE9, 0, 0, 0, 0,                   /* 77 jmp skip               */
    };

    if (!base) {
        return 0;
    }
    gate = base + rva;
    continue_at = gate + 6;
    /* The stock jne's own target, 0x3DD past the instruction for the verified site. */
    skip_at = continue_at + 0x3DD;
    for (candidate = ((uintptr_t)base & ~(uintptr_t)0xFFFF) - 0x10000;
         candidate > (uintptr_t)base - 0x70000000u; candidate -= 0x10000) {
        stub = (unsigned char*)VirtualAlloc((void*)candidate, 0x1000, MEM_RESERVE | MEM_COMMIT,
                                            PAGE_EXECUTE_READWRITE);
        if (stub) {
            break;
        }
    }
    if (!stub) {
        return 0;
    }
    memset(stub, 0xCC, 0x1000);
    memcpy(stub, body, sizeof(body));
    displacement = (int32_t)(continue_at - (stub + 6));
    memcpy(stub + 2, &displacement, 4);
    displacement = (int32_t)(continue_at - (stub + 64));
    memcpy(stub + 60, &displacement, 4);
    displacement = (int32_t)(skip_at - (stub + 82));
    memcpy(stub + 78, &displacement, 4);
    jitter_patch.stub = stub;
    jitter_patch.stub_width = (volatile LONG*)(void*)(stub + 88);
    jitter_patch.stub_height = (volatile LONG*)(void*)(stub + 92);
    jitter_patch.stub_view = (void* volatile*)(void*)(stub + 96);
    jitter_patch.stub_allowed = (volatile LONG*)(void*)(stub + 104);
    jitter_patch.stub_denied = (volatile LONG*)(void*)(stub + 108);
    jitter_patch.stub_last_denied = (volatile LONG*)(void*)(stub + 112);
    *jitter_patch.stub_width = 0;
    *jitter_patch.stub_height = 0;
    *jitter_patch.stub_view = NULL;
    *jitter_patch.stub_allowed = 0;
    *jitter_patch.stub_denied = 0;
    *jitter_patch.stub_last_denied = 0;
    FlushInstructionCache(GetCurrentProcess(), stub, 0x1000);
    return 1;
}

/* Keep the stub's main-view size in step with the reconstruction, on the render thread. */
static void update_jitter_main_view(void)
{
    unsigned long width = 0;
    unsigned long height = 0;
    if (!jitter_patch.stub) {
        return;
    }
    rsf_bridge_view_size(&width, &height);
    /* Log decision deltas every 20 maintenance calls. */
    {
        static int ticks = 0;
        static LONG last_allowed = 0;
        static LONG last_denied = 0;
        if (++ticks >= 20) {
            const LONG allowed = *jitter_patch.stub_allowed;
            const LONG denied = *jitter_patch.stub_denied;
            ticks = 0;
            if (allowed != last_allowed || denied != last_denied) {
                note("jitter stub: %ld views jittered, %ld turned away, last turned away at %ld "
                     "wide or high",
                     allowed - last_allowed, denied - last_denied,
                     (long)*jitter_patch.stub_last_denied);
                last_allowed = allowed;
                last_denied = denied;
            }
        }
    }
    if ((LONG)width != *jitter_patch.stub_width || (LONG)height != *jitter_patch.stub_height) {
        InterlockedExchange(jitter_patch.stub_width, (LONG)width);
        InterlockedExchange(jitter_patch.stub_height, (LONG)height);
        if (width != 0) {
            note("jitter: main view is %lux%lu; views of any other size stay unjittered", width,
                 height);
        }
    }
}

/* Idempotently write the expected open/closed gate bytes and report each transition.
   State changes only after the guarded patch succeeds. */
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
    /* The six byte canonical nop, so the fall-through path is reached for every view. */
    static const uint8_t open_all[6] = {0x66, 0x0F, 0x1F, 0x44, 0x00, 0x00};
    /* Or a jump to the main-view stub, and a nop. */
    uint8_t open_main[6] = {0xE9, 0, 0, 0, 0, 0x90};
    const uint8_t* open = open_all;
    if (jitter_patch.stub) {
        unsigned char* gate_address = (unsigned char*)GetModuleHandleW(NULL) + jitter_patch.rva;
        const int32_t displacement = (int32_t)(jitter_patch.stub - (gate_address + 5));
        memcpy(open_main + 1, &displacement, 4);
        open = open_main;
    }
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

/* Render-thread action for automatic reconstruction startup and explicit jitter overrides. */
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

/* Verify the site by opening the gate once; mode 1 closes it until reconstruction starts.
   Use the main-view stub only for the researched RVA, otherwise retain the all-view experiment. */
static void apply_jitter_patch(void)
{
    jitter_patch.mode = (int)read_number("RSF_ENABLE_JITTER", 1);
    if (jitter_patch.mode == 0) {
        return;
    }
    jitter_patch.rva = read_number("RSF_JITTER_RVA", 0x112b1f3);
    jitter_patch.site_verified = 1;
    jitter_patch.enabled = 0;
    /* The stub knows this site's registers and its jump target, so it is used there only. */
    if (jitter_patch.rva == 0x112b1f3 && read_number("RSF_JITTER_ALL_VIEWS", 0) == 0) {
        if (build_jitter_stub(jitter_patch.rva)) {
            note("jitter: main view only, through a stub at %p", (void*)jitter_patch.stub);
        } else {
            note("jitter: no memory near the game for the main-view stub, so every view is "
                 "jittered as before");
        }
    }

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

/* Opt-in dynamic-mesh velocity experiment: remove only the blend-mode rejection in
   FVelocityDrawingPolicyFactory::DrawDynamicMesh. Material-domain, movable and shader-support
   checks remain. Static draw-list geometry is unaffected; this patch alone does not establish
   usable vectors. Untested against the game; see docs/research/ue418-hook-map.md. */
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

/* Request paired diagnostics and wait up to five seconds for observer completion. The
   wait is a diagnostic cost; this function can also be invoked from a render-thread action. */
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

    /* Give each requested snapshot a distinct prefix for cross-scene comparison. */
    static unsigned capture_index = 0;
    const unsigned index = capture_index++;

    char prefix[MAX_PATH * 2];
    snprintf(prefix, sizeof(prefix), "%s\\capture%02u", observe_directory, index);
    /* Queue GPU readback for the next Present instead of accessing the context here. */
    const rsf_observer_result requested = rsf_observer_request_dump(prefix, RSF_DUMP_VIEW_VELOCITY);

    char sizes[MAX_PATH * 2];
    snprintf(sizes, sizeof(sizes), "%s\\capture%02u-buffer-sizes.csv", observe_directory, index);
    rsf_observer_write_buffer_sizes(sizes);

    note("capture %u requested (result %d), %u distinct constant buffer sizes seen", index,
         (int)requested, status.distinct_buffer_sizes);

    /* Pair reconstruction output and observer inputs under the same capture index. */
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

/* Resolve the sibling AC7 plugin and provide the executable fingerprint for guarded
   native preparation. Preparation does not activate its hooks. */
static int prepare_native_game(void)
{
    wchar_t path[MAX_PATH]; char sha256[65];
    const DWORD length = GetModuleFileNameW((HMODULE)&__ImageBase, path, MAX_PATH);
    wchar_t* separator = length && length < MAX_PATH ? wcsrchr(path, L'\\') : NULL;
    if (!separator || !executable_sha256(sha256) ||
        (size_t)(separator - path) + 1 + wcslen(L"ReScaleFrame.Game.AC7.dll") >= MAX_PATH) return 0;
    wcscpy(separator + 1, L"ReScaleFrame.Game.AC7.dll");
    return rsf_bridge_prepare_game(path, sha256, observer_note, NULL);
}
static int accept_ac7_window(void* user, void* window)
{
    wchar_t name[64]; DWORD process = 0;
    (void)user;
    GetWindowThreadProcessId((HWND)window, &process);
    if (process != GetCurrentProcessId() || !GetClassNameW((HWND)window, name, 64) || wcscmp(name, L"UnrealWindow") != 0) return 0;
    // Native preparation checks decrypted hook bytes and the executable fingerprint before
    // the cold presentation host creates any D3D12 objects. A refusal keeps the original chain.
    return prepare_native_game();
}
static void fg_latency_event(void* user, const rsf_observer_present_event* event)
{ (void)user; rsf_native_fg_present(event); }
/* Configure the cold presentation host before the first game swapchain. Saved provider
   choice overrides the INI fallback; native game preparation validates accepted AC7 windows.
   Unsupported installation preserves the original D3D11 presentation path. */
static void start_generation(void)
{
    char directory[MAX_PATH * 2];
    const DWORD fallback = read_number("RSF_FG_ENABLE", 0) ? read_number("RSF_FG_BACKEND", RSF_FG_BACKEND_DLSS) : 0;
    const DWORD backend = rsf_fg_choice_start(preference_path, fallback,
        (1u << RSF_FG_BACKEND_DLSS) | (1u << RSF_FG_BACKEND_FSR3) | (1u << RSF_FG_BACKEND_FSR4) | (1u << RSF_FG_BACKEND_XESS));
    const char* variable = backend == RSF_FG_BACKEND_XESS ? "RSF_XESS_BIN" :
        backend == RSF_FG_BACKEND_FSR3 ? "RSF_FSR3_BIN" : backend == RSF_FG_BACKEND_FSR4 ? "RSF_FSR4_BIN" : "RSF_STREAMLINE_BIN";
    const char* relative = backend == RSF_FG_BACKEND_XESS ? "ReScaleFrame\\xess" :
        backend == RSF_FG_BACKEND_FSR3 || backend == RSF_FG_BACKEND_FSR4 ? "ReScaleFrame\\fidelityfx" : "ReScaleFrame\\streamline";
    if (!read_text(variable, directory, sizeof(directory)) &&
        !((backend == RSF_FG_BACKEND_FSR3 || backend == RSF_FG_BACKEND_FSR4) && read_text("RSF_FFX_BIN", directory, sizeof(directory))) &&
        !beside_this_module(relative, directory, sizeof(directory))) {
        note("frame generation: runtime directory unavailable; retaining D3D11 presentation"); return;
    }
    rsf_native_fg_options options = {0}; options.struct_size = sizeof(options);
    options.mode = read_number("RSF_FG_MODE", RSF_FG_FIXED);
    options.generated_frames = read_number("RSF_FG_GENERATED", 1);
    options.reflex_mode = backend == RSF_FG_BACKEND_DLSS ? read_number("RSF_REFLEX_MODE", RSF_REFLEX_ON) : RSF_REFLEX_OFF;
    /* A limit on rendered frames, applied before generation. FPS wins over the interval form. */
    {
        const DWORD limit_fps = read_number("RSF_FRAME_LIMIT_FPS", 0);
        options.frame_limit_us = limit_fps ? (1000000u + limit_fps / 2u) / limit_fps : read_number("RSF_REFLEX_LIMIT_US", 0);
    }
    options.debug = read_number("RSF_FG_DEBUG", 0);
    rsf_native_fg_options_set(&options);
    /* RSF_REFLEX_ASYNC=1 restores the sleep that overlaps the previous frame's Present, for comparison. */
    rsf_native_fg_reflex_ordering_set(read_number("RSF_REFLEX_ASYNC", 0));
    rsf_native_fg_set_log(observer_note, NULL);
    rsf_d3d11_present_setup setup = {0}; setup.struct_size = sizeof(setup);
    setup.runtime_directory_utf8 = directory; setup.development_runtime = read_number("RSF_FG_DEVELOPMENT", 0);
    setup.log = observer_note; setup.accept_window = accept_ac7_window;
    setup.before_present = rsf_bridge_present_hook(); setup.present_event = rsf_bridge_present_event_hook();
    setup.latency_event = fg_latency_event; setup.prepare = rsf_native_fg_prepare; setup.retire = rsf_native_fg_retire;
    setup.debug_timing = options.debug;
    setup.backend = backend; setup.max_generated_frames = UINT32_MAX;
    setup.runtime_switching = 1;
    {
        char streamline[MAX_PATH * 2] = {0}, fsr3[MAX_PATH * 2] = {0}, fsr4[MAX_PATH * 2] = {0}, xess[MAX_PATH * 2] = {0};
        if (!read_text("RSF_STREAMLINE_BIN", streamline, sizeof(streamline))) beside_this_module("ReScaleFrame\\streamline", streamline, sizeof(streamline));
        if (!read_text("RSF_FSR3_BIN", fsr3, sizeof(fsr3)) && !read_text("RSF_FFX_BIN", fsr3, sizeof(fsr3))) beside_this_module("ReScaleFrame\\fidelityfx", fsr3, sizeof(fsr3));
        if (!read_text("RSF_FSR4_BIN", fsr4, sizeof(fsr4)) && !read_text("RSF_FFX_BIN", fsr4, sizeof(fsr4))) beside_this_module("ReScaleFrame\\fidelityfx", fsr4, sizeof(fsr4));
        if (!read_text("RSF_XESS_BIN", xess, sizeof(xess))) beside_this_module("ReScaleFrame\\xess", xess, sizeof(xess));
        setup.streamline_directory_utf8 = streamline; setup.fsr3_directory_utf8 = fsr3;
        setup.fsr4_directory_utf8 = fsr4; setup.xess_directory_utf8 = xess;
        if (!rsf_d3d11_present_install(&setup)) note("frame generation: interception refused; retaining D3D11 presentation");
    }
    note("frame generation: cold startup requested, backend=%lu mode=%lu generated=%lu Reflex=%lu directory=%s (%s)",
        (unsigned long)backend, (unsigned long)options.mode, (unsigned long)options.generated_frames, (unsigned long)options.reflex_mode,
        directory, setup.development_runtime ? "development" : "production");
    note("frame limit before generation: %lu us between rendered frames (0 is unlimited)",
        (unsigned long)options.frame_limit_us);
    note(read_number("RSF_REFLEX_ASYNC", 0) ?
        "Reflex ordering: sleep may precede the previous Present (RSF_REFLEX_ASYNC=1)" :
        "Reflex ordering: previous Present, then one sleep, then input; submit end before PresentStart");
}
/* Worker sequence: create logging, install observation/cold presentation, await two stable
   entropy samples, apply guarded patches, prepare native hooks, then publish startup_ready.
   Executable dumping adds bounded late module sampling and an automatic snapshot. */
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
    /* Create the selected output leaf; custom parent directories must already exist. */
    CreateDirectoryA(directory, NULL);

    MultiByteToWideChar(CP_ACP, 0, directory, -1, log_path, MAX_PATH);
    wcscat(log_path, L"\\rsf-dump.log");

    /* The observer detours Present while preserving the Steam overlay hook chain. */
    if (GetModuleHandleW(L"gameoverlayrenderer64.dll")) {
        note("steam overlay: gameoverlayrenderer64.dll is loaded; the observer detours Present "
             "over its hook and keeps it in the chain, so both run");
    }

    strncpy(observe_directory, directory, MAX_PATH - 1);
    observe_directory[MAX_PATH - 1] = '\0';
    start_observer();
    start_generation();

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
        /* The same decryption readiness check without writing the licensed executable to disk. */
        const ULONGLONG deadline = GetTickCount64() + 180000;
        unsigned stable = 0;
        do {
            result = rsf_measure_module_code(NULL, &entropy);
            if (result != RSF_DUMP_OK) {
                break;
            }
            stable = entropy < 7.0 ? stable + 1 : 0;
            if (stable >= 2) {
                break;
            }
            result = RSF_DUMP_ERROR_STILL_ENCRYPTED;
            Sleep(250);
        } while (GetTickCount64() < deadline);
    }
    if (result != RSF_DUMP_OK) {
        note("startup: executable did not become ready, result %d; DLSS remains inactive", (int)result);
        return 0;
    }
    /* Patch only after stable decryption readiness; earlier writes can be overwritten by unpacking. */
    apply_jitter_patch();
    apply_translucent_velocity_patch();
    /* Before the scale patch, because the scale it settles on depends on whether these applied. */
    translucency_depth_conformed = apply_translucency_depth_patches();
    apply_separate_translucency_patch();
    note("native AC7 plugin preparation: %s", prepare_native_game() ? "ready" : "refused");
    if (read_number("RSF_MOTION_CAPTURE", 0) != 0) {
        rsf_ac7_motion_capture_install();
    }
    InterlockedExchange(&startup_ready, 1);
    if (!dump_module) {
        note("startup: renderer patches prepared; waiting for a presented frame to start DLSS");
        return 0;
    }

    note("result %d, entropy %d.%03d, sections %u, imports %u, iat references %u, bytes %llu",
         (int)result, (int)report.code_entropy,
         (int)((report.code_entropy - (int)report.code_entropy) * 1000),
         report.sections_written, report.imports_described, report.iat_references,
         (unsigned long long)report.bytes_written);

    /* Sample late module loads at bounded marks after decryption; startup imports do not
       establish which renderer the game uses. */
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

    /* Request a late automatic snapshot when executable dumping was explicitly enabled. */
    report_and_dump();
    return 0;
}

/* Compatibility renderer: use round(8 * (100 / screen_percentage)^2) temporal samples,
   with a minimum of eight. Reuse the validated value offset from the float cvar; verify both
   integer thread copies before writing. Native ownership manages its own sampling. */
static void set_jitter_sequence_length(float percentage, uint32_t value_offset)
{
    if (rsf_bridge_native_owned()) return;
    static int32_t previous_samples = 8;
    if (percentage <= 0.0f) {
        return;
    }
    const float ratio = 100.0f / percentage;
    int32_t samples = (int32_t)(8.0f * ratio * ratio + 0.5f);
    if (samples < 8) {
        samples = 8;
    }

    rsf_dump_result result =
        rsf_console_set_int("r.TemporalAASamples", previous_samples, samples, value_offset,
                            read_number("RSF_CONSOLE_SINGLETON_RVA", 0x3a8b290),
                            read_number("RSF_CONSOLE_FIND_SLOT", 0x90));
    if (result != RSF_DUMP_OK && previous_samples != 8) {
        result = rsf_console_set_int("r.TemporalAASamples", 8, samples, value_offset,
                                    read_number("RSF_CONSOLE_SINGLETON_RVA", 0x3a8b290),
                                    read_number("RSF_CONSOLE_FIND_SLOT", 0x90));
    }
    if (result == RSF_DUMP_OK) {
        previous_samples = samples;
        note("jitter sequence length set to %d at object offset 0x%lx", (int)samples,
             (unsigned long)value_offset);
    } else {
        /* A differing default or layout must fail the expected-value check. */
        note("jitter sequence length NOT set (result %d), offset 0x%lx did not hold 8 twice",
             (int)result, (unsigned long)value_offset);
    }
}

/* Last successfully applied engine screen percentage. Change the engine cvar so buffer
   allocation, view rectangles, shader sizes and jitter scaling remain consistent. */
static unsigned long requested_scale_percent;

/* Change the expected previous/reset cvar value, then update jitter and layer scale.
   Return zero on refusal and log a read-only object probe for diagnosis. */
static int set_screen_percentage(float value)
{
    uint32_t offset = 0;
    const float previous = requested_scale_percent ? (float)requested_scale_percent : 100.0f;
    rsf_dump_result result =
        rsf_console_set_float("r.ScreenPercentage", previous, value,
                              read_number("RSF_CONSOLE_SINGLETON_RVA", 0x3a8b290),
                              read_number("RSF_CONSOLE_FIND_SLOT", 0x90), &offset);
    if (result != RSF_DUMP_OK && previous != 100.0f) {
        result = rsf_console_set_float("r.ScreenPercentage", 100.0f, value,
                                      read_number("RSF_CONSOLE_SINGLETON_RVA", 0x3a8b290),
                                      read_number("RSF_CONSOLE_FIND_SLOT", 0x90), &offset);
    }
    if (result == RSF_DUMP_OK) {
        requested_scale_percent = (unsigned long)value;
        note("screen percentage set to %d, value found at object offset 0x%lx", (int)value,
             (unsigned long)offset);
        set_jitter_sequence_length(value, offset);
        set_separate_translucency_scale();
        return 1;
    }

    /* Probe on refusal to record the manager, variable and leading values for layout research. */
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
    return 0;
}

/* Start on the render thread after decryption and the first presenting device are ready. */
static void start_dlss(void)
{
    char directory[MAX_PATH * 2];
    char fsr_directories[3][MAX_PATH * 2] = {{0}};
    char xess_directory[MAX_PATH * 2] = {0};
    char sdk_override[MAX_PATH * 2] = {0};
    DWORD width, height;

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

    /* Use the observed presented extent; refuse startup before it is known. */
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
         (unsigned long)height, (unsigned long)preferred_quality);
    if (!read_text("RSF_FFX_BIN", fsr_directories[0], sizeof(fsr_directories[0])))
        beside_this_module("ReScaleFrame\\fidelityfx", fsr_directories[0], sizeof(fsr_directories[0]));
    memcpy(fsr_directories[1], fsr_directories[0], sizeof(fsr_directories[0]));
    memcpy(fsr_directories[2], fsr_directories[0], sizeof(fsr_directories[0]));
    if (read_text("RSF_FSR2_BIN", sdk_override, sizeof(sdk_override))) strcpy(fsr_directories[0], sdk_override);
    if (read_text("RSF_FSR3_BIN", sdk_override, sizeof(sdk_override))) strcpy(fsr_directories[1], sdk_override);
    if (read_text("RSF_FSR4_BIN", sdk_override, sizeof(sdk_override))) strcpy(fsr_directories[2], sdk_override);
    if (!read_text("RSF_XESS_BIN", xess_directory, sizeof(xess_directory)))
        beside_this_module("ReScaleFrame\\xess", xess_directory, sizeof(xess_directory));
    rsf_bridge_set_sdk_directories(fsr_directories[0], fsr_directories[1], fsr_directories[2], xess_directory);
    rsf_bridge_start(directory, width, height, preferred_quality, observer_note,
                     NULL);
    /* Interface panels drawn with a jittered view are moved back by that view's own jitter. */
    rsf_bridge_set_unjitter((int)read_number("RSF_UI_UNJITTER", 1));
    rsf_bridge_set_translucency_unjitter((int)read_number("RSF_TRANSLUCENCY_UNJITTER", 1));
    rsf_bridge_report();
}

/* Restore the requested compatibility scale only when the engine resets its cvar to 100.
   Mission loading can reapply graphics settings; an expected-value write leaves other values
   untouched. Called by render-thread maintenance. */
static void keep_render_scale(void)
{
    uint32_t offset = 0;
    if (!requested_scale_percent) {
        return;
    }
    /* Preserve the last user/preset choice across engine settings resets. */
    const float value = requested_scale_percent
                            ? (float)requested_scale_percent
                            : (float)read_number("RSF_SCREEN_PERCENTAGE", 50);
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

/* Overlay actions run from the bridge Present callback on the graphics owner thread.
   They apply settings, queue diagnostics and expose the last successfully applied scale. */

static void action_start_backend(void)
{
    start_dlss();
}

/* Address of the aligned float immediate installed by the scale patch, null until verified.
   Update with one aligned 32-bit store after changing page protection and flush the instruction
   cache. The graphics owner supplies subsequent scale changes. */
static volatile uint32_t* translucency_scale_slot;
static float translucency_scale_now;

/* Derive layer size from output-relative RSF_TRANSLUCENCY_TARGET (default 100%; zero
   matches the scene). RSF_TRANSLUCENCY_SCALE overrides it with a scene-relative multiplier
   expressed as percent. Clamp to 0.25..4 and to at most 1 unless all depth/view patches succeeded.
   Allocation padding follows engine alignment; unjittering is a separate control. */
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

    /* Before a preset has actually changed the game, its scene is still native. */
    render_percent = requested_scale_percent ? requested_scale_percent : 100;
    if (render_percent == 0) {
        render_percent = 100;
    }
    target = read_number("RSF_TRANSLUCENCY_TARGET", 100);

    if (override_percent != 0) {
        scale.value = (float)override_percent / 100.0f;
    } else {
        scale.value = rsf_ac7_translucency_scale(render_percent, target);
    }
    /* The engine clamps its own console value to 100 and this patch is downstream of that clamp,
       so the bound has to be here. */
    if (scale.value < 0.25f) {
        scale.value = 0.25f;
    }
    /* Stock UE4.18 borrows scene depth at scale >= 1, producing an invalid colour/depth
       pair above 1. Permit enlargement only after allocation, bind, resolve and view consumers
       all use the corrected scale gates. A refusal leaves a lower-resolution visible layer. */
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
         "%d%% of native",
         (int)(scale.value * 100.0f), render_percent,
         (int)(scale.value * (float)render_percent));
}

/* Replace the verified 15-byte scale-selection window in SetSeparateTranslucencyBufferSize
   with nop; mov eax,<float bits>; movd xmm1,eax. rax is dead across this window; downstream code
   uses xmm1 for width, height and stored scale. The six-byte leading nop aligns the immediate
   at RVA+7 for later atomic stores. A nondefault RVA skips the default expected-byte check;
   an unaligned immediate is left at the startup scale. Evidence: docs/research/ue418-hook-map.md. */
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

/* Make separate translucency borrow scene depth/view only at scale == 1. The eleven
   verified sites cover allocation, binding, resolves, mesh view selection and shader depth
   consumers. Preflight every window, apply together, and attempt rollback on write failure.
   Only complete success permits scale > 1. Evidence: docs/research/ac7-consumer-session.md. */
static int apply_translucency_depth_patches(void)
{
    static const struct {
        DWORD rva;
        uint8_t count;
        uint8_t expected[9];
        uint8_t replacement[9];
        const char* what;
    } sites[] = {
        {0x1168f6f, 2, {0x76, 0x0E}, {0x74, 0x0E}, "the depth allocation and fill"},
        {0x1097a0c, 2, {0x76, 0x17}, {0x74, 0x17}, "the depth bind"},
        {0x109d03a, 4, {0x44, 0x0F, 0x47, 0xF9}, {0x44, 0x0F, 0x45, 0xF9}, "the snapshot resolve"},
        {0x109d061, 2, {0x76, 0x17}, {0x74, 0x17}, "the resolve"},
        {0x11583a6, 9, {0x0F, 0x2F, 0x80, 0x20, 0x02, 0x00, 0x00, 0x76, 0x0E},
                        {0x0F, 0x2F, 0x80, 0x20, 0x02, 0x00, 0x00, 0x74, 0x0E}, "the layer view selection"},
        {0x1025b12, 9, {0x0F, 0x2F, 0x86, 0x20, 0x02, 0x00, 0x00, 0x76, 0x04},
                        {0x0F, 0x2F, 0x86, 0x20, 0x02, 0x00, 0x00, 0x74, 0x04}, "shader depth selection 1"},
        {0x10294d2, 9, {0x0F, 0x2F, 0x86, 0x20, 0x02, 0x00, 0x00, 0x76, 0x04},
                        {0x0F, 0x2F, 0x86, 0x20, 0x02, 0x00, 0x00, 0x74, 0x04}, "shader depth selection 2"},
        {0x102ada2, 9, {0x0F, 0x2F, 0x86, 0x20, 0x02, 0x00, 0x00, 0x76, 0x04},
                        {0x0F, 0x2F, 0x86, 0x20, 0x02, 0x00, 0x00, 0x74, 0x04}, "shader depth selection 3"},
        {0x102c672, 9, {0x0F, 0x2F, 0x86, 0x20, 0x02, 0x00, 0x00, 0x76, 0x04},
                        {0x0F, 0x2F, 0x86, 0x20, 0x02, 0x00, 0x00, 0x74, 0x04}, "shader depth selection 4"},
        {0x102e792, 9, {0x0F, 0x2F, 0x86, 0x20, 0x02, 0x00, 0x00, 0x76, 0x04},
                        {0x0F, 0x2F, 0x86, 0x20, 0x02, 0x00, 0x00, 0x74, 0x04}, "shader depth selection 5"},
        {0x1031902, 9, {0x0F, 0x2F, 0x86, 0x20, 0x02, 0x00, 0x00, 0x76, 0x04},
                        {0x0F, 0x2F, 0x86, 0x20, 0x02, 0x00, 0x00, 0x74, 0x04}, "shader depth selection 6"},
    };
    uint8_t previous[9] = {0};
    unsigned int i;

    /* Writing the identical bytes performs the patch helper's validated range/expected-byte
       check without changing behaviour. No gate is changed until every site passes. */
    for (i = 0; i < sizeof(sites) / sizeof(sites[0]); ++i) {
        const rsf_dump_result result = rsf_patch_code(sites[i].rva, sites[i].expected, sites[i].count,
                                                     sites[i].expected, sites[i].count, previous);
        if (result != RSF_DUMP_OK) {
            note("translucency preflight: %s at rva 0x%lx refused, result %d; no gates changed",
                 sites[i].what, (unsigned long)sites[i].rva, (int)result);
            return 0;
        }
    }
    for (i = 0; i < sizeof(sites) / sizeof(sites[0]); ++i) {
        const rsf_dump_result result =
            rsf_patch_code(sites[i].rva, sites[i].replacement, sites[i].count, sites[i].expected,
                           sites[i].count, previous);
        if (result != RSF_DUMP_OK) {
            unsigned int undo = i;
            while (undo > 0) {
                --undo;
                const rsf_dump_result restored = rsf_patch_code(sites[undo].rva, sites[undo].expected,
                    sites[undo].count, sites[undo].replacement, sites[undo].count, previous);
                if (restored != RSF_DUMP_OK) {
                    note("translucency rollback: %s failed, result %d", sites[undo].what, (int)restored);
                }
            }
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

static int action_set_render_scale(unsigned long percent)
{
    if (percent == 0 || percent > 100) {
        return 0;
    }
    return set_screen_percentage((float)percent);
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

/* Compatibility depth mismatch policy: 0 drops depth testing, 1 forwards the incompatible
   pair, 2 refuses target promotion. This diagnostic choice does not provide a matching depth. */
static unsigned long action_reinsert_depth_policy(void)
{
    return read_number("RSF_REINSERT_DEPTH", 0);
}

static unsigned long action_startup_ready(void)
{
    return (unsigned long)InterlockedCompareExchange(&startup_ready, 0, 0);
}

/* Persist the accepted enable/preset pair; failed persistence leaves session state active. */
static void save_preferences(void)
{
    if (!rsf_preferences_write(preference_path, preferred_enabled, preferred_quality)) {
        note("settings: could not save preferences; the current session still uses them");
    }
}

static void action_save_quality(unsigned long quality)
{
    if (quality <= 4) {
        preferred_quality = (uint32_t)quality;
        save_preferences();
    }
}

static void action_save_enabled(unsigned long enabled)
{
    preferred_enabled = enabled != 0;
    save_preferences();
}

/* Compatibility maintenance at Present: refresh main-view dimensions and repair engine
   screen-percentage resets at most twice per second. */
static void action_maintain_renderer(void)
{
    static ULONGLONG last_check;
    if (rsf_bridge_native_owned()) return;
    update_jitter_main_view();
    const ULONGLONG now = GetTickCount64();
    if (now - last_check >= 500) {
        last_check = now;
        keep_render_scale();
    }
}

/* Copy the callback table into the bridge, then apply INI/environment defaults and saved
   per-user enable/quality preferences. Missing profile storage leaves the defaults usable. */
static void register_overlay_actions(void)
{
    rsf_bridge_actions actions;
    memset(&actions, 0, sizeof(actions));
    actions.start_backend = action_start_backend;
    actions.reinsert_depth_policy = action_reinsert_depth_policy;
    actions.set_render_scale = action_set_render_scale;
    actions.trigger_dump = action_trigger_dump;
    actions.trigger_capture = action_trigger_capture;
    actions.capture_count = action_capture_count;
    actions.render_scale_percent = action_render_scale_percent;
    actions.set_jitter = action_set_jitter;
    actions.jitter_open = action_jitter_open;
    actions.jitter_available = action_jitter_available;
    actions.startup_ready = action_startup_ready;
    actions.save_quality = action_save_quality;
    actions.save_enabled = action_save_enabled;
    actions.maintain_renderer = action_maintain_renderer;
    rsf_bridge_set_actions(&actions);
    preferred_enabled = read_number("RSF_DLSS_ENABLE", 1) != 0;
    rsf_dlss_pipeline_set_colour_correction(read_number("RSF_DLSS_COLOUR_CORRECTION", 0) != 0);
    rsf_dlss_pipeline_set_colour_transport(read_number("RSF_DLSS_TONEMAP", 1) != 0);
    preferred_quality = read_number("RSF_DLSS_QUALITY", 3);
    if (preferred_quality > 4) {
        preferred_quality = 3;
    }
    const DWORD length = GetEnvironmentVariableW(L"LOCALAPPDATA", preference_path, MAX_PATH);
    if (length > 0 && length < MAX_PATH) {
        wcscat(preference_path, L"\\ReScaleFrame");
        if (CreateDirectoryW(preference_path, NULL) || GetLastError() == ERROR_ALREADY_EXISTS) {
            wcscat(preference_path, L"\\AC7.ini");
            rsf_preferences_read(preference_path, &preferred_enabled, &preferred_quality);
        } else {
            preference_path[0] = L'\0';
        }
    } else {
        preference_path[0] = L'\0';
    }
    rsf_bridge_select_quality(preferred_quality);
    rsf_bridge_set_enabled((int)preferred_enabled);
}

/* One fatal-exception report per process. The vectored handler leaves exception handling
   to the game (EXCEPTION_CONTINUE_SEARCH); dbghelp and minidump support load on demand. */
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

/* Borrowed crash context passed to a reporting worker so stack-overflow diagnostics use
   a fresh stack. The handler waits at most 30 seconds; callers must not treat this as an owned
   copy or as a guarantee that the worker has completed after timeout. */
struct crash_report {
    EXCEPTION_POINTERS* info;
    DWORD thread;
};

/* Best-effort symbol walk and minidump using the borrowed exception context. The report
   remains diagnostic: the handler continues the original exception search. */
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
            /* Bound the walk while retaining enough depth to expose common recursion cycles. */
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

/* Filter fatal codes, claim the single report atomically, and wait for its worker for
   at most 30 seconds. A timed-out worker is not cancelled and still holds borrowed pointers. */
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
        /* If worker creation fails, report inline except on the exhausted stack-overflow stack. */
        note("crash: exception 0x%08lx on thread %lu, no thread could be made, describing inline",
             code, report.thread);
        if (code != EXCEPTION_STACK_OVERFLOW) {
            crash_report_worker(&report);
        }
    }
    return EXCEPTION_CONTINUE_SEARCH;
}

/* AC7 process-attach entry used directly as DllMain or dispatched by the generic shim.
   Installs the crash handler, optional pre-device capture, and a detached preparation worker.
   Process-lifetime hooks are not torn down from loader-lock notifications. */
BOOL WINAPI rsf_ac7_proxy_process_event(HINSTANCE instance, DWORD reason, LPVOID reserved)
{
    (void)reserved;
    if (reason == DLL_PROCESS_ATTACH) {
        DisableThreadLibraryCalls(instance);
        /* First, before anything that could fault. */
        AddVectoredExceptionHandler(1, on_fatal_exception);
#if RSF_HAVE_FRAME_CAPTURE
        /* Capture opt-in must load before device creation; ordinary renderer work stays on workers. */
        start_capture_support();
#endif
        /* The worker owns decryption polling and preparation; Present owns graphics activation. */
        HANDLE thread = CreateThread(NULL, 0, dump_worker, NULL, 0, NULL);
        if (thread) {
            CloseHandle(thread);
        }

    }
    return TRUE;
}

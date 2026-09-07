/* SPDX-License-Identifier: GPL-3.0-only */
/* Research proxy: loads with the game, forwards DirectInput8Create to the real dinput8, and
   dumps the main module once its code section stops looking like ciphertext.

   dinput8 is the carrier because AC7 imports exactly one function from it and nothing else in the
   process does, so the forwarding surface is one export and DXVK is left alone. */

#include <rescaleframe/module_dump.h>

#include <rescaleframe/d3d11_observer.h>
#include <rescaleframe/texture_dump.h>

#include "dlss_bridge.h"

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

#if RSF_HAVE_FRAME_CAPTURE
/* RenderDoc has to be loaded before the graphics device exists, so this runs on the carrier's own
   attach rather than on the worker thread. It only loads a library and reads two variables. */
static void start_capture_support(void)
{
    char library[MAX_PATH];
    if (GetEnvironmentVariableA("RSF_RENDERDOC_DLL", library, MAX_PATH) == 0) {
        return;
    }
    char prefix[MAX_PATH];
    if (GetEnvironmentVariableA("RSF_CAPTURE_PREFIX", prefix, MAX_PATH) == 0) {
        prefix[0] = '\0';
    }
    rsf_capture_initialise(library, prefix[0] ? prefix : NULL);
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

static DWORD read_number(const char* name, DWORD fallback)
{
    char text[64];
    if (GetEnvironmentVariableA(name, text, sizeof(text)) == 0) {
        return fallback;
    }
    const long value = strtol(text, NULL, 10);
    return value > 0 ? (DWORD)value : fallback;
}

/* The observer's progress lines go to the same log as everything else. note() opens, writes and
   closes per line, which is what makes the last line before a crash survive it. */
static void observer_note(void* user, const char* message)
{
    (void)user;
    note("%s", message);
}

/* Installed before the module dump rather than after it. The creation hook only sees textures
   made after it is in place, and the target we are after is allocated during engine startup. */
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
   refuses rather than corrupting an instruction. */
static void apply_jitter_patch(void)
{
    if (read_number("RSF_ENABLE_JITTER", 0) == 0) {
        return;
    }
    const DWORD rva = read_number("RSF_JITTER_RVA", 0x112b1f3);
    /* jne rel32 */
    const uint8_t expected[6] = {0x0F, 0x85, 0xDD, 0x03, 0x00, 0x00};
    /* The six byte canonical nop, so the fall-through path is reached. */
    const uint8_t replacement[6] = {0x66, 0x0F, 0x1F, 0x44, 0x00, 0x00};
    uint8_t previous[6] = {0};

    const rsf_dump_result result =
        rsf_patch_code(rva, replacement, sizeof(replacement),
                       rva == 0x112b1f3 ? expected : NULL, rva == 0x112b1f3 ? sizeof(expected) : 0,
                       previous);
    if (result == RSF_DUMP_OK) {
        note("jitter gate patched at rva 0x%lx, was %02x %02x %02x %02x %02x %02x",
             (unsigned long)rva, previous[0], previous[1], previous[2], previous[3], previous[4],
             previous[5]);
    } else {
        note("jitter gate NOT patched at rva 0x%lx, result %d (expected bytes did not match?)",
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
    if (GetEnvironmentVariableA("RSF_DUMP_DIR", directory, MAX_PATH) == 0) {
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
    if (GetEnvironmentVariableA("RSF_STREAMLINE_BIN", directory, sizeof(directory)) == 0) {
        note("RSF_STREAMLINE_BIN is not set, so there is nothing to load DLSS from");
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
    }
}

static DWORD WINAPI observe_worker(LPVOID parameter)
{
    (void)parameter;
    int was_down = 0;
    int scale_down = 0;
    int dlss_down = 0;
    int show_down = 0;
    int reinsert_down = 0;
    int ticks = 0;
    int running = 1;
    while (running) {
        const int dlss = (GetAsyncKeyState(VK_F8) & 0x8000) != 0;
        if (dlss && !dlss_down) {
            start_dlss();
        }
        dlss_down = dlss;

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

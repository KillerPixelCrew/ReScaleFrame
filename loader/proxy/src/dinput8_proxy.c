/* SPDX-License-Identifier: GPL-3.0-only */
/* Research proxy: loads with the game, forwards DirectInput8Create to the real dinput8, and
   dumps the main module once its code section stops looking like ciphertext.

   dinput8 is the carrier because AC7 imports exactly one function from it and nothing else in the
   process does, so the forwarding surface is one export and DXVK is left alone. */

#include <rescaleframe/module_dump.h>

#include <rescaleframe/d3d11_observer.h>
#include <rescaleframe/texture_dump.h>

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
    options.minimum_width = read_number("RSF_OBSERVE_MIN_WIDTH", 1024);
    options.capacity = read_number("RSF_OBSERVE_CAPACITY", 8);
    /* A range rather than the stock size: AC7 runs a vendor branch, and the first run showed no
       buffer of the stock 2640 bytes at all. Everything in range is retained per distinct size,
       and every size seen is counted, which is what identifies the right one. */
    options.constant_buffer_min_bytes = read_number("RSF_VIEW_CB_MIN", 1024);
    options.constant_buffer_max_bytes = read_number("RSF_VIEW_CB_MAX", 8192);

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

static DWORD WINAPI observe_worker(LPVOID parameter)
{
    (void)parameter;
    int was_down = 0;
    int running = 1;
    while (running) {
        const int down = (GetAsyncKeyState(VK_F10) & 0x8000) != 0;
        if (down && !was_down && observe_directory[0]) {
            report_and_dump();
        }
        was_down = down;
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

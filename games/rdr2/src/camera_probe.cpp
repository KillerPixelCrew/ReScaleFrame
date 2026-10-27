// SPDX-License-Identifier: GPL-3.0-only
// Capture the worker-local view at the native DLSS/FSR2 boundary. These raw snapshots do not
// assert a matrix layout: the matching disassembly and numeric validation establish that later.
#include "camera_probe.h"
#include <windows.h>
#include <MinHook.h>
#include <intrin.h>
#include <atomic>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <mutex>

namespace {
using Upscale = bool (*)(void*, void*, bool);
Upscale fsr_original{}, dlss_original{};
const unsigned char* image{};
wchar_t output[MAX_PATH]{};
wchar_t request[MAX_PATH]{};
std::mutex capture_mutex;
std::atomic<ULONGLONG> next_poll{};
unsigned remaining{}, capture_id{}, sample{};
constexpr unsigned capture_frames = 64;
void (*native_init)(uint64_t){};
void (*native_push)(uint64_t){};
uint64_t* (*native_call)(){};
void (*script_wait)(DWORD){};

void script_reference()
{
    for (;;) {
        bool active{};
        unsigned sequence{};
        {
            std::lock_guard<std::mutex> lock(capture_mutex);
            active = remaining != 0;
            sequence = capture_id;
        }
        if (active) {
            // ScriptHook calls belong to its script fiber, never the upscaler's job worker.
            // Vector results use 8-byte slots, as confirmed by the installed exports and the
            // reference C# interop decoder. These are comparison values, not the render source.
            float position[3]{}, rotation[3]{}, fov{};
            native_init(0x5352E025EC2B416Full);
            auto* result = reinterpret_cast<float*>(native_call());
            for (unsigned i = 0; i != 3; ++i) position[i] = result[i * 2];
            native_init(0x602685BD85DD26CAull);
            native_push(2);
            result = reinterpret_cast<float*>(native_call());
            for (unsigned i = 0; i != 3; ++i) rotation[i] = result[i * 2];
            native_init(0x04AF77971E508F6Aull);
            std::memcpy(&fov, native_call(), sizeof(fov));
            LARGE_INTEGER qpc{};
            QueryPerformanceCounter(&qpc);
            uint32_t frame{};
            std::memcpy(&frame, image + 0x542904c, sizeof(frame));
            wchar_t filename[MAX_PATH]{};
            swprintf_s(filename, L"%s\\script-%lu-%03u.jsonl", output, GetCurrentProcessId(), sequence);
            FILE* f = _wfopen(filename, L"ab");
            if (f) {
                fprintf(f, "{\"frame\":%u,\"qpc\":%llu,\"thread\":%lu,\"position\":[%.9g,%.9g,%.9g],"
                    "\"rotation_order_2\":[%.9g,%.9g,%.9g],\"fov_degrees\":%.9g}\n", frame,
                    static_cast<unsigned long long>(qpc.QuadPart), GetCurrentThreadId(),
                    position[0], position[1], position[2], rotation[0], rotation[1], rotation[2], fov);
                fclose(f);
            }
        }
        script_wait(0);
    }
}

bool read(const void* source, void* destination, size_t bytes) noexcept
{
    SIZE_T copied{};
    return source && ReadProcessMemory(GetCurrentProcess(), source, destination, bytes, &copied) && copied == bytes;
}

struct Snapshot {
    char magic[8];
    uint32_t version, size, capture, index, provider, thread;
    uint64_t tick, qpc, image_base, tls_block, view, context, parameters, return_rva;
    uint32_t frame_counter, tls_index, valid_view, valid_parameters;
    unsigned char view_bytes[0x800];
    unsigned char parameter_bytes[0x100];
    uint32_t render_width, render_height;
    uint64_t job, draw_context;
    unsigned char job_bytes[0x80];
};
static_assert(sizeof(Snapshot) == 2568);

void capture(unsigned provider, void* context, void* parameters, void* caller) noexcept
{
    // Only one invocation owns an armed sequence. No graphics state is changed and no game
    // pointers survive this callback; each record owns a copy of the bytes read at this boundary.
    std::lock_guard<std::mutex> lock(capture_mutex);
    const auto now = GetTickCount64();
    if (!remaining && now >= next_poll.load(std::memory_order_relaxed)) {
        next_poll.store(now + 500, std::memory_order_relaxed);
        if (GetFileAttributesW(request) != INVALID_FILE_ATTRIBUTES && DeleteFileW(request)) {
            remaining = capture_frames;
            ++capture_id;
            sample = 0;
        }
    }
    if (!remaining) return;
    Snapshot s{};
    std::memcpy(s.magic, "RSFCAM1", 8);
    s.version = 2;
    s.size = sizeof(s);
    s.capture = capture_id;
    s.index = sample++;
    s.provider = provider;
    s.thread = GetCurrentThreadId();
    s.tick = now;
    LARGE_INTEGER qpc{};
    QueryPerformanceCounter(&qpc);
    s.qpc = qpc.QuadPart;
    s.image_base = reinterpret_cast<uint64_t>(image);
    s.context = reinterpret_cast<uint64_t>(context);
    s.parameters = reinterpret_cast<uint64_t>(parameters);
    s.return_rva = reinterpret_cast<uint64_t>(caller) - s.image_base;
    read(image + 0x542904c, &s.frame_counter, sizeof(s.frame_counter));
    read(image + 0x5b403e0, &s.tls_index, sizeof(s.tls_index));
    uint64_t tls_array = __readgsqword(0x58);
    if (s.tls_index < 4096 && read(reinterpret_cast<void*>(tls_array + s.tls_index * 8), &s.tls_block, 8)) {
        read(reinterpret_cast<void*>(s.tls_block + 0x14d8), &s.view, 8);
        read(reinterpret_cast<void*>(s.tls_block + 0x14f0), &s.draw_context, 8);
        read(reinterpret_cast<void*>(s.tls_block + 0x1ce8), &s.job, 8);
        read(reinterpret_cast<void*>(s.job), s.job_bytes, sizeof(s.job_bytes));
        s.valid_view = read(reinterpret_cast<void*>(s.view), s.view_bytes, sizeof(s.view_bytes));
    }
    s.valid_parameters = read(parameters, s.parameter_bytes, sizeof(s.parameter_bytes));
    uint64_t color{};
    uint16_t extent[2]{};
    read(parameters, &color, sizeof(color));
    if (color && read(reinterpret_cast<void*>(color + 0x18), extent, sizeof(extent))) {
        s.render_width = extent[0];
        s.render_height = extent[1];
    }
    wchar_t filename[MAX_PATH]{};
    if (swprintf_s(filename, L"%s\\camera-%lu-%03u-%03u.bin", output, GetCurrentProcessId(), capture_id, s.index) > 0) {
        FILE* f = _wfopen(filename, L"wb");
        if (f) {
            fwrite(&s, sizeof(s), 1, f);
            fclose(f);
        }
    }
    --remaining;
}

bool fsr_hook(void* context, void* parameters, bool option)
{
    capture(2, context, parameters, _ReturnAddress());
    return fsr_original(context, parameters, option);
}
bool dlss_hook(void* context, void* parameters, bool option)
{
    capture(1, context, parameters, _ReturnAddress());
    return dlss_original(context, parameters, option);
}
}

bool rsf_rdr2_install_camera_probe(const wchar_t* directory) noexcept
{
    image = reinterpret_cast<const unsigned char*>(GetModuleHandleW(nullptr));
    if (!image || !directory || wcslen(directory) > MAX_PATH - 64) return false;
    const auto* dos = reinterpret_cast<const IMAGE_DOS_HEADER*>(image);
    if (dos->e_magic != IMAGE_DOS_SIGNATURE) return false;
    const auto* nt = reinterpret_cast<const IMAGE_NT_HEADERS64*>(image + dos->e_lfanew);
    if (nt->Signature != IMAGE_NT_SIGNATURE || nt->FileHeader.TimeDateStamp != 0x65eec92e ||
        nt->OptionalHeader.SizeOfImage != 0x7397600) return false;
    constexpr unsigned char fsr_bytes[]{0x48,0x83,0x3a,0x00,0x4c,0x8b,0xca,0x74,0x27,0x48,0x83,0x7a,0x18,0x00,0x74,0x20};
    constexpr unsigned char dlss_bytes[]{0x8b,0x81,0xa8,0x00,0x00,0x00,0x4c,0x8d,0x0c,0x80,0x4a,0x8d,0x0c,0xc9,0x48,0x83};
    auto* fsr = const_cast<unsigned char*>(image + 0x7ba10c);
    auto* dlss = const_cast<unsigned char*>(image + 0x7bb60c);
    if (std::memcmp(fsr, fsr_bytes, sizeof(fsr_bytes)) || std::memcmp(dlss, dlss_bytes, sizeof(dlss_bytes))) return false;
    wcscpy_s(output, directory);
    swprintf_s(request, L"%s\\camera.request", output);
    auto status = MH_Initialize();
    if (status != MH_OK && status != MH_ERROR_ALREADY_INITIALIZED) return false;
    if (MH_CreateHook(fsr, reinterpret_cast<void*>(&fsr_hook), reinterpret_cast<void**>(&fsr_original)) != MH_OK) return false;
    if (MH_CreateHook(dlss, reinterpret_cast<void*>(&dlss_hook), reinterpret_cast<void**>(&dlss_original)) != MH_OK) {
        MH_RemoveHook(fsr);
        return false;
    }
    // Publish only complete hooks; restore the first entry if the second enable fails.
    if (MH_EnableHook(fsr) != MH_OK) return false;
    if (MH_EnableHook(dlss) == MH_OK) return true;
    MH_DisableHook(fsr);
    return false;
}

bool rsf_rdr2_install_camera_reference(void* self, const wchar_t* library) noexcept
{
    if (!self || !library || !output[0] || !fsr_original || !dlss_original) return false;
    HMODULE module = LoadLibraryW(library);
    if (!module) return false;
    using Register = void (*)(HMODULE, void (*)());
    const auto register_script = reinterpret_cast<Register>(GetProcAddress(module, "?scriptRegister@@YAXPEAUHINSTANCE__@@P6AXXZ@Z"));
    native_init = reinterpret_cast<decltype(native_init)>(GetProcAddress(module, "?nativeInit@@YAX_K@Z"));
    native_push = reinterpret_cast<decltype(native_push)>(GetProcAddress(module, "?nativePush64@@YAX_K@Z"));
    native_call = reinterpret_cast<decltype(native_call)>(GetProcAddress(module, "?nativeCall@@YAPEA_KXZ"));
    script_wait = reinterpret_cast<decltype(script_wait)>(GetProcAddress(module, "?scriptWait@@YAXK@Z"));
    if (!register_script || !native_init || !native_push || !native_call || !script_wait) return false;
    register_script(static_cast<HMODULE>(self), script_reference);
    return true;
}

void rsf_rdr2_request_camera_capture() noexcept
{
    std::lock_guard<std::mutex> lock(capture_mutex);
    if (!output[0] || remaining) return;
    remaining = capture_frames;
    ++capture_id;
    sample = 0;
}

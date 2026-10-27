// SPDX-License-Identifier: GPL-3.0-only
#include "native_execution.h"
#include "native_camera.h"
#include <rescaleframe/log.h>
#include <MinHook.h>
#include <d3d11.h>
#include <wrl/client.h>
#include <atomic>
#include <cstring>
#include <limits>
#include <memory>
#include <mutex>
#include <unordered_map>

namespace rsf::wingman {
namespace {
using Microsoft::WRL::ComPtr;
struct Rect { int32_t left, top, right, bottom; };
struct TaaParameters {
    uint32_t kind;
    uint8_t fast, render_targetable, downsample, padding;
    uint32_t downsample_format;
    Rect input, output;
    int32_t divisor;
    void* depth;
    void* motion;
    void* colour;
    void* metadata;
};
static_assert(offsetof(TaaParameters, input) == 0xc && offsetof(TaaParameters, output) == 0x1c);
static_assert(offsetof(TaaParameters, colour) == 0x40 && sizeof(TaaParameters) == 0x50);
using AddTaa = void* (*)(void*, void*, const void*, const TaaParameters*, const void*, void*);
using AddCompute = void (*)(void*, void*, uint8_t, const void*, const void*, const void*);
using ExecutePass = void (*)(void*, void*);
AddTaa original_taa = nullptr;
AddCompute original_compute = nullptr;
ExecutePass original_execute = nullptr;
void* sites[3]{};
bool installed[3]{};
std::atomic<bool> accepting{false};
std::atomic<uint32_t> executing{0}, queued{0};
struct Entry {
    Entry() { executing.fetch_add(1, std::memory_order_acq_rel); }
    ~Entry() { executing.fetch_sub(1, std::memory_order_release); }
};
std::mutex guard;
rsf_game_host_services services{};
struct Pending {
    rsf_game_render_pass pass{};
    void* rdg[4]{};
};
thread_local Pending* construction = nullptr;
std::unordered_map<const void*, Pending> pending;

bool copy(void* output, const void* input, size_t bytes) noexcept
{
    if (!input) return false;
#if defined(_MSC_VER)
    __try { std::memcpy(output, input, bytes); return true; }
    __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
#else
    MEMORY_BASIC_INFORMATION page{};
    if (!VirtualQuery(input, &page, sizeof(page)) || page.State != MEM_COMMIT ||
        (page.Protect & (PAGE_GUARD | PAGE_NOACCESS)) ||
        uintptr_t(input) + bytes > uintptr_t(page.BaseAddress) + page.RegionSize) return false;
    std::memcpy(output, input, bytes); return true;
#endif
}
template<class T> bool read(const void* base, size_t offset, T& value) noexcept
{ return base && copy(&value, static_cast<const unsigned char*>(base) + offset, sizeof(value)); }

// UE4.27 inserts GetSizeXYZ before GetNativeResource. Calling the 4.18 slot would
// use a different return ABI; this getter is slot 0x38 in the matched branch.
bool texture_from_rdg(void* rdg, ID3D11Texture2D** out) noexcept
{
    *out = nullptr;
#if defined(_MSC_VER)
    __try {
#endif
        void* rhi = nullptr; void** table = nullptr;
        if (!read(rdg, 0x10, rhi) || !read(rhi, 0, table) || !table) return false;
        using Native = void* (*)(void*);
        auto* native = reinterpret_cast<IUnknown*>(reinterpret_cast<Native>(table[7])(rhi));
        return native && SUCCEEDED(native->QueryInterface(__uuidof(ID3D11Texture2D), reinterpret_cast<void**>(out)));
#if defined(_MSC_VER)
    } __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
#endif
}

struct Command;
using CommandFunction = void (*)(Command*, void*, void*);
struct Command {
    const CommandFunction* vtable;
    Command* next = nullptr;
    rsf_game_render_pass pass{};
    rsf_game_host_services host{};
    ComPtr<ID3D11Texture2D> textures[4];
};
static_assert(offsetof(Command, vtable) == 0 && offsetof(Command, next) == 8);

void execute(Command* command, void* native_list, void*) noexcept
{
    std::unique_ptr<Command> owned(command);
    try {
        ComPtr<ID3D11Device> device;
        command->textures[0]->GetDevice(&device);
        ComPtr<ID3D11DeviceContext> context;
        device->GetImmediateContext(&context);
        D3D11_TEXTURE2D_DESC desc[4]{};
        for (uint32_t i = 0; i < 4; ++i) command->textures[i]->GetDesc(&desc[i]);
        static std::atomic<ULONGLONG> next_log{0};
        const auto now = GetTickCount64();
        auto previous = next_log.load(std::memory_order_relaxed);
        if (now >= previous && next_log.compare_exchange_strong(previous, now + 1000))
            rsf::say(command->host.log, command->host.user,
                "PW GPU temporal list=%p history=%p colour=%ux%u/%u depth=%ux%u/%u motion=%ux%u/%u output=%ux%u/%u camera=%u frame=%llu",
                native_list, reinterpret_cast<void*>(uintptr_t(command->pass.history_key)),
                desc[0].Width, desc[0].Height, unsigned(desc[0].Format),
                desc[1].Width, desc[1].Height, unsigned(desc[1].Format),
                desc[2].Width, desc[2].Height, unsigned(desc[2].Format),
                desc[3].Width, desc[3].Height, unsigned(desc[3].Format), command->pass.camera_valid,
                static_cast<unsigned long long>(command->pass.native_frame));
        // The native fallback's dispatch precedes this command. COM leases preserve these
        // allocations; later graph writes execute after this callback on the same RHI stream.
        // A host must order any external GPU work before returning ownership to that stream.
        if (accepting.load(std::memory_order_acquire) && command->host.render_pass) {
            command->host.render_pass(command->host.user, context.Get(), &command->pass, 1);
            command->host.render_pass(command->host.user, context.Get(), &command->pass, 0);
        }
    } catch (...) { /* An optional consumer never interrupts native command retirement. */ }
    owned.reset();
    queued.fetch_sub(1, std::memory_order_release);
}
const CommandFunction command_vtable[] = {execute};

bool append(void* list, Command* command) noexcept
{
    Command** tail = nullptr; uint32_t count = 0; uint8_t in_execution = 0;
    if (!read(list, 8, tail) || !read(list, 0x14, count) || !read(list, 0x10, in_execution) ||
        in_execution || !tail || *tail || count == std::numeric_limits<uint32_t>::max()) return false;
    *tail = command;
    tail = &command->next;
    std::memcpy(static_cast<unsigned char*>(list) + 8, &tail, sizeof(tail));
    ++count;
    std::memcpy(static_cast<unsigned char*>(list) + 0x14, &count, sizeof(count));
    return true;
}

void* add_taa(void* result, void* graph, const void* view, const TaaParameters* parameters,
              const void* previous_history, void* output_history)
{
    Entry entered;
    Pending metadata{};
    TaaParameters input{};
    auto* parent = construction;
    if (accepting.load(std::memory_order_acquire) && copy(&input, parameters, sizeof(input)) &&
        input.kind < 3 && input.divisor == 1) {
        metadata.pass.struct_size = sizeof(metadata.pass); metadata.pass.role = RSF_GAME_RENDER_SR;
        metadata.pass.family_key = uintptr_t(graph); metadata.pass.view_key = uintptr_t(view);
        std::memcpy(metadata.pass.render_rect, &input.input, sizeof(input.input));
        std::memcpy(metadata.pass.output_rect, &input.output, sizeof(input.output));
        read(view, 0x1680, metadata.pass.history_key);
        read(view, 0x4fa4, metadata.pass.camera.jitter_pixels);
        uint8_t cut = 0; read(view, 0xd90, cut);
        uintptr_t old_history = 0; read(previous_history, 0, old_history);
        metadata.pass.flags = cut || !old_history ? RSF_GAME_RENDER_RESET : 0;
        read_camera(view, metadata.pass);
        metadata.rdg[0] = input.colour; metadata.rdg[1] = input.depth; metadata.rdg[2] = input.motion;
        construction = &metadata;
    } else construction = nullptr;
    auto* returned = original_taa(result, graph, view, parameters, previous_history, output_history);
    construction = parent;
    return returned;
}

void add_compute(void* graph, void* name, uint8_t flags, const void* shader,
                 const void* parameters, const void* groups)
{
    Entry entered;
    try { if (construction && accepting.load(std::memory_order_acquire)) {
        Pending copied = *construction;
        void* uav = nullptr;
        if (read(parameters, 0x278, uav) && read(uav, 0x28, copied.rdg[3])) {
            std::lock_guard<std::mutex> lock(guard);
            if (pending.size() < 256 || pending.contains(parameters)) pending[parameters] = copied;
        }
    } } catch (...) { /* Keep the complete native graph if packet allocation is refused. */ }
    original_compute(graph, name, flags, shader, parameters, groups);
}

void execute_pass(void* pass, void* list)
{
    Entry entered;
    std::unique_ptr<Command> command;
    const void* parameters = nullptr;
    try { if (read(pass, 0xf0, parameters)) {
        Pending selected{};
        bool found = false;
        {
            std::lock_guard<std::mutex> lock(guard);
            const auto entry = pending.find(parameters);
            if (entry != pending.end()) { selected = entry->second; pending.erase(entry); found = true; }
            if (found && accepting.load(std::memory_order_relaxed)) {
                command = std::make_unique<Command>(); command->vtable = command_vtable;
                command->pass = selected.pass; command->host = services;
                command->pass.session_id = services.session_id;
            }
        }
        if (command) {
            for (uint32_t i = 0; i < 4; ++i) {
                ID3D11Texture2D* texture = nullptr;
                if (!texture_from_rdg(selected.rdg[i], &texture)) { command.reset(); break; }
                command->textures[i].Attach(texture);
            }
        }
    } } catch (...) { command.reset(); }
    original_execute(pass, list);
    if (command) {
        command->pass.color_input = command->textures[0].Get(); command->pass.depth = command->textures[1].Get();
        command->pass.motion = command->textures[2].Get(); command->pass.color_output = command->textures[3].Get();
        if (queued.load(std::memory_order_acquire) < 256) {
            queued.fetch_add(1, std::memory_order_acq_rel);
            if (append(list, command.get())) command.release();
            else queued.fetch_sub(1, std::memory_order_release);
        }
    }
}
}

bool execution_prepare(const rsf_game_host_services& host) noexcept try
{
    constexpr uintptr_t rvas[] = {0x5c44910,0x5c2d4a0,0x5c4c9c0};
    constexpr unsigned char bytes[3][16] = {
        {0x48,0x8b,0xc4,0x48,0x89,0x58,0x08,0x55,0x56,0x57,0x41,0x54,0x41,0x55,0x41,0x56},
        {0x48,0x89,0x5c,0x24,0x08,0x48,0x89,0x6c,0x24,0x10,0x48,0x89,0x74,0x24,0x18,0x48},
        {0x48,0x83,0xec,0x38,0xf2,0x0f,0x10,0x81,0x08,0x01,0x00,0x00,0x4c,0x8d,0x4c,0x24}};
    auto* base = reinterpret_cast<unsigned char*>(GetModuleHandleW(nullptr));
    for (uint32_t i=0;i<3;++i) {
        unsigned char found[16]{}; sites[i]=base+rvas[i];
        if (!copy(found,sites[i],sizeof(found)) || std::memcmp(found,bytes[i],sizeof(found))) return false;
    }
    void* detours[] = {reinterpret_cast<void*>(&add_taa),reinterpret_cast<void*>(&add_compute),reinterpret_cast<void*>(&execute_pass)};
    void** originals[] = {reinterpret_cast<void**>(&original_taa),reinterpret_cast<void**>(&original_compute),reinterpret_cast<void**>(&original_execute)};
    for (uint32_t i=0;i<3;++i) if (!*originals[i] && MH_CreateHook(sites[i],detours[i],originals[i])!=MH_OK) return false;
    std::lock_guard<std::mutex> lock(guard); services=host;
    return true;
}
catch (...) { return false; }

bool execution_start() noexcept
{
    for (uint32_t i=0;i<3;++i) {
        if (!installed[i] && MH_EnableHook(sites[i])!=MH_OK) { execution_stop(); return false; }
        installed[i]=true;
    }
    accepting.store(true,std::memory_order_release); return true;
}
void execution_quiesce() noexcept { accepting.store(false,std::memory_order_release); }
bool execution_stop() noexcept try
{
    execution_quiesce();
    for (uint32_t i=0;i<3;++i) if (installed[i]) {
        if (MH_DisableHook(sites[i])!=MH_OK) return false;
        installed[i]=false;
    }
    if (executing.load(std::memory_order_acquire) || queued.load(std::memory_order_acquire)) return false;
    std::lock_guard<std::mutex> lock(guard); pending.clear(); services={}; return true;
}
catch (...) { return false; }
}

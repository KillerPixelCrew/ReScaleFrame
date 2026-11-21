// SPDX-License-Identifier: MIT
#include "native_execution.h"
#include "native_read.h"
#include <rescaleframe/log.h>
#include <MinHook.h>
#include <d3d11.h>
#include <wrl/client.h>
#include <limits>
#include <memory>

namespace rsf::wingman {
struct Packet {
    rsf_game_render_pass pass;
    uintptr_t rdg[4];
};
static_assert(sizeof(Packet) == sizeof(rsf_game_render_pass) + 4 * sizeof(uintptr_t));
}

extern "C" {
void rsf_wingman_exec_enter() noexcept;
void rsf_wingman_exec_leave() noexcept;
const rsf::wingman::Packet* rsf_wingman_exec_taa_begin(uintptr_t, uintptr_t, uintptr_t, uintptr_t, rsf::wingman::Packet*) noexcept;
void rsf_wingman_exec_taa_end(const rsf::wingman::Packet*) noexcept;
void rsf_wingman_exec_record_compute(uintptr_t) noexcept;
bool rsf_wingman_exec_take(uintptr_t, rsf::wingman::Packet*, rsf_game_host_services*) noexcept;
bool rsf_wingman_exec_reserve() noexcept;
void rsf_wingman_exec_retire() noexcept;
bool rsf_wingman_exec_diagnostic_due(uint64_t) noexcept;
void rsf_wingman_exec_report(const rsf_game_host_services*, void*, const rsf_game_render_pass*) noexcept;
}

namespace rsf::wingman {
namespace {
using Microsoft::WRL::ComPtr;
struct TaaParameters;
using AddTaa = void* (*)(void*, void*, const void*, const TaaParameters*, const void*, void*);
using AddCompute = void (*)(void*, void*, uint8_t, const void*, const void*, const void*);
using ExecutePass = void (*)(void*, void*);
AddTaa original_taa = nullptr;
AddCompute original_compute = nullptr;
ExecutePass original_execute = nullptr;

struct Entry {
    Entry() { rsf_wingman_exec_enter(); }
    ~Entry() { rsf_wingman_exec_leave(); }
};

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
        Native getter = nullptr;
        if (!read(table, 7 * sizeof(void*), getter) || !getter) return false;
        auto* native = reinterpret_cast<IUnknown*>(getter(rhi));
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
        if (rsf_wingman_exec_diagnostic_due(GetTickCount64())) {
            D3D11_TEXTURE2D_DESC desc[4]{};
            for (uint32_t i = 0; i < 4; ++i) command->textures[i]->GetDesc(&desc[i]);
            rsf::say(command->host.log, command->host.user,
                "PW GPU temporal list=%p history=%p colour=%ux%u/%u depth=%ux%u/%u motion=%ux%u/%u output=%ux%u/%u camera=%u frame=%llu",
                native_list, reinterpret_cast<void*>(uintptr_t(command->pass.history_key)),
                desc[0].Width, desc[0].Height, unsigned(desc[0].Format),
                desc[1].Width, desc[1].Height, unsigned(desc[1].Format),
                desc[2].Width, desc[2].Height, unsigned(desc[2].Format),
                desc[3].Width, desc[3].Height, unsigned(desc[3].Format), command->pass.camera_valid,
                static_cast<unsigned long long>(command->pass.native_frame));
        }
        rsf_wingman_exec_report(&command->host, context.Get(), &command->pass);
    } catch (...) { /* An optional consumer never interrupts native command retirement. */ }
    owned.reset();
    rsf_wingman_exec_retire();
}
const CommandFunction command_vtable[] = {execute};

bool append(void* list, Command* command) noexcept
{
    Command** tail = nullptr; uint32_t count = 0; uint8_t in_execution = 0;
    Command* existing = nullptr;
    if (!read(list, 8, tail) || !read(list, 0x14, count) || !read(list, 0x10, in_execution) ||
        in_execution || !read(tail, 0, existing) || existing || count == std::numeric_limits<uint32_t>::max()) return false;
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
    Packet storage;
    struct Restore {
        const Packet* previous;
        ~Restore() { rsf_wingman_exec_taa_end(previous); }
    } restore{rsf_wingman_exec_taa_begin(uintptr_t(graph), uintptr_t(view), uintptr_t(parameters),
        uintptr_t(previous_history), &storage)};
    return original_taa(result, graph, view, parameters, previous_history, output_history);
}

void add_compute(void* graph, void* name, uint8_t flags, const void* shader,
    const void* parameters, const void* groups)
{
    Entry entered;
    rsf_wingman_exec_record_compute(uintptr_t(parameters));
    original_compute(graph, name, flags, shader, parameters, groups);
}

void execute_pass(void* pass, void* list)
{
    Entry entered;
    std::unique_ptr<Command> command;
    try {
        Packet selected;
        rsf_game_host_services host;
        if (rsf_wingman_exec_take(uintptr_t(pass), &selected, &host)) {
            command = std::make_unique<Command>();
            command->vtable = command_vtable;
            command->pass = selected.pass;
            command->host = host;
            for (uint32_t i = 0; i < 4; ++i) {
                ID3D11Texture2D* texture = nullptr;
                if (!texture_from_rdg(reinterpret_cast<void*>(selected.rdg[i]), &texture)) {
                    command.reset();
                    break;
                }
                command->textures[i].Attach(texture);
            }
        }
    } catch (...) { command.reset(); }
    original_execute(pass, list);
    if (!command || !rsf_wingman_exec_reserve()) return;
    command->pass.color_input = command->textures[0].Get();
    command->pass.depth = command->textures[1].Get();
    command->pass.motion = command->textures[2].Get();
    command->pass.color_output = command->textures[3].Get();
    if (append(list, command.get())) command.release();
    else rsf_wingman_exec_retire();
}
}

bool create_execution_hook(uint32_t index, uintptr_t target) noexcept
{
    void* detours[] = {reinterpret_cast<void*>(&add_taa), reinterpret_cast<void*>(&add_compute),
        reinterpret_cast<void*>(&execute_pass)};
    void** originals[] = {reinterpret_cast<void**>(&original_taa), reinterpret_cast<void**>(&original_compute),
        reinterpret_cast<void**>(&original_execute)};
    return index < 3 && MH_CreateHook(reinterpret_cast<void*>(target), detours[index], originals[index]) == MH_OK;
}

void report_execution(const rsf_game_host_services* host, void* context, const rsf_game_render_pass* pass) noexcept
{
    // The native fallback's dispatch already ran. Keep both host calls on the same RHI stream.
    try {
        if (host->render_pass) {
            host->render_pass(host->user, context, pass, 1);
            host->render_pass(host->user, context, pass, 0);
        }
    } catch (...) {}
}
}

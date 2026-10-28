// SPDX-License-Identifier: GPL-3.0-only
#include "native_renderer.h"
#include "native_inputs.h"
#include <windows.h>
#include <d3d12.h>
#include <dxgi1_6.h>
#include <wrl/client.h>
#include <MinHook.h>
#include <intrin.h>
#include <array>
#include <atomic>
#include <cstring>
#include <mutex>
#include <memory>
#include <cstdio>
#include <unordered_map>

namespace {
using Microsoft::WRL::ComPtr;
using namespace rsf_gta5_enhanced;
rsf_game_host_services host{};
uintptr_t image{};
std::atomic<bool> active{};
std::atomic<uint32_t> callbacks{};
std::atomic<uint64_t> submissions{};
bool prepared{};
struct Scope { Scope() { ++callbacks; } ~Scope() { --callbacks; } };
void log(const char* text) { if (host.log) host.log(host.user, text); }
bool read(const void* address, void* out, size_t size)
{
    SIZE_T copied{};
    return address && ReadProcessMemory(GetCurrentProcess(), address, out, size, &copied) && copied == size;
}
uint32_t viewport(const void* value)
{
    uint32_t result = UINT32_MAX;
    if (value) read(static_cast<const unsigned char*>(value) + 32, &result, sizeof(result));
    return result;
}
uint64_t frame(const void* token)
{
    uintptr_t table{}, function{};
    if (!read(token, &table, sizeof(table)) || !read(reinterpret_cast<void*>(table), &function, sizeof(function))) return 0;
    using Index = uint32_t (*)(const void*);
    return uint64_t(reinterpret_cast<Index>(function)(token)) + 1;
}
struct Input {
    ComPtr<ID3D12Resource> resource;
    uint32_t state{}, width{}, height{};
};
struct Frame {
    uint64_t id{};
    NativeConstants constants{};
    bool have_constants{};
    float seconds{};
    Input color, output, depth, motion, exposure, scene, ui;
};
std::array<Frame, 64> frames;
std::mutex frame_guard, render_guard;
LARGE_INTEGER frequency{}, last_input{};
uint64_t last_input_frame{};
float previous_jitter[2]{};
uint64_t previous_frame{};
std::atomic<bool> native_gap{};
Frame& slot(uint64_t id)
{
    auto& value = frames[id % frames.size()];
    if (value.id != id) { value = {}; value.id = id; }
    return value;
}
using SetConstants = int (*)(const void*, const void*, const void*);
using SetTags = int (*)(const void*, const void*, const NativeTag*, uint32_t, void*);
using Evaluate = int (*)(uint32_t, const void*, const void* const*, uint32_t, void*);
using NewToken = int (*)(void**, const uint32_t*);
using FeatureFunction = int (*)(uint32_t, const char*, void**);
using Options = int (*)(const void*, const void*);
SetConstants original_constants{};
SetTags original_tags{};
Evaluate original_evaluate{};
NewToken original_token{};
FeatureFunction original_feature{};
Options original_fg_options{};
using SetDevice = int (*)(void*);
SetDevice original_set_device{};
using AttachGraphics = int32_t (*)(void*, void*);
using NativeObject = void* (*)(void*, void*);
using GraphicsReady = uint32_t (*)(void*);
AttachGraphics attach_device{}, attach_factory{};
NativeObject native_object_fn{};
GraphicsReady graphics_ready_fn{};
ComPtr<IUnknown> native_object(void* object)
{
    ComPtr<IUnknown> result;
    if (object && native_object_fn) result.Attach(static_cast<IUnknown*>(native_object_fn(host.user, object)));
    return result;
}
using CreateFactory = HRESULT (WINAPI *)(UINT, REFIID, void**);
CreateFactory original_factory{};
int set_device(void* device)
{
    const auto result = original_set_device(device);
    if (!result && active.load() && device && attach_device) {
        auto native = native_object(device);
        const auto attached = attach_device(host.user, native.Get());
        log(attached ? "GTA native device observer refused." : "GTA native device observer attached.");
    }
    return result;
}
HRESULT WINAPI create_factory(UINT flags, REFIID iid, void** output)
{
    const auto result = original_factory(flags, iid, output);
    if (SUCCEEDED(result) && output && *output && active.load() && attach_factory) {
        auto native = native_object(*output);
        const auto attached = attach_factory(host.user, native.Get());
        log(attached ? "GTA native factory observer refused." : "GTA native factory observer attached.");
    }
    return result;
}
int constants(const void* pointer, const void* token, const void* view)
{
    Scope callback;
    const auto id = frame(token);
    NativeConstants copy{};
    if (active.load() && id && viewport(view) == 0 && read(pointer, &copy, sizeof(copy))) {
        {
            std::lock_guard lock(frame_guard);
            auto& value = slot(id); value.constants = copy; value.have_constants = copy.header.next == nullptr;
        }
        if (native_gap.load()) { copy.reset = 1; return original_constants(&copy, token, view); }
    }
    return original_constants(pointer, token, view);
}
int tags(const void* token, const void* view, const NativeTag* pointer, uint32_t count, void* commands)
{
    Scope callback;
    const auto id = frame(token);
    if (active.load() && id && viewport(view) == 0 && count <= 64) {
        std::lock_guard lock(frame_guard);
        auto& value = slot(id);
        for (uint32_t i = 0; i < count; ++i) {
            NativeTag tag{}; NativeResource resource{};
            if (!read(pointer + i, &tag, sizeof(tag))) continue;
            Input* input{};
            switch (tag.type) {
            case 0: input = &value.depth; break;
            case 1: input = &value.motion; break;
            case 2: input = &value.scene; break;
            case 3: input = &value.color; break;
            case 4: input = &value.output; break;
            case 13: input = &value.exposure; break;
            case 23: input = &value.ui; break;
            default: break;
            }
            if (input) {
                if (!tag.resource || !read(tag.resource, &resource, sizeof(resource)) || resource.type != 0 || !resource.native) {
                    *input = {}; continue;
                }
                auto native = native_object(resource.native);
                ComPtr<ID3D12Resource> texture;
                if (SUCCEEDED(native.As(&texture))) *input = {texture, resource.state,
                    tag.width ? tag.width : resource.width, tag.height ? tag.height : resource.height};
            }
        }
    }
    return original_tags(token, view, pointer, count, commands);
}
int new_token(void** token, const uint32_t* index)
{
    Scope callback;
    const auto result = original_token(token, index);
    if (!result && token && *token && active.load()) {
        const auto id = frame(*token);
        LARGE_INTEGER now{}; QueryPerformanceCounter(&now);
        std::lock_guard lock(frame_guard);
        if (id && id != last_input_frame) {
            auto& value = slot(id);
            if (last_input.QuadPart && last_input_frame + 1 == id)
                value.seconds = static_cast<float>(double(now.QuadPart - last_input.QuadPart) / frequency.QuadPart);
            last_input = now; last_input_frame = id;
        }
    }
    return result;
}
int evaluate(uint32_t feature, const void* token, const void* const* inputs, uint32_t count, void* commands)
{
    Scope callback;
    if (feature != 0 || !active.load()) return original_evaluate(feature, token, inputs, count, commands);
    const auto id = frame(token);
    Frame value{};
    { std::lock_guard lock(frame_guard); if (id && frames[id % frames.size()].id == id) value = frames[id % frames.size()]; }
    std::lock_guard rendering(render_guard);
    const bool graphics_ready = graphics_ready_fn && graphics_ready_fn(host.user);
    if (graphics_ready && value.have_constants && value.color.resource && value.output.resource &&
        value.depth.resource && value.motion.resource && commands && host.process_upscale) {
        const auto output = value.output.resource->GetDesc();
        rsf_game_render_pass pass{};
        pass.struct_size = sizeof(pass); pass.role = RSF_GAME_RENDER_SR; pass.session_id = host.session_id;
        pass.family_key = pass.view_key = pass.history_key = 1;
        pass.pass_key = pass.native_frame = pass.source_frame_id = id;
        pass.native_frame_context = const_cast<void*>(token);
        pass.resource_generation = 1; pass.flags = RSF_GAME_RENDER_PRIMARY | RSF_GAME_RENDER_NO_FG;
        if (value.constants.reset == 1 || previous_frame + 1 != id) pass.flags |= RSF_GAME_RENDER_RESET;
        pass.color_input = value.color.resource.Get(); pass.color_output = value.output.resource.Get();
        pass.depth = value.depth.resource.Get(); pass.motion = value.motion.resource.Get(); pass.exposure = value.exposure.resource.Get();
        pass.screen = RSF_SCREEN_UNKNOWN;
        pass.render_rect[2] = static_cast<int32_t>(value.color.width); pass.render_rect[3] = static_cast<int32_t>(value.color.height);
        pass.output_rect[2] = static_cast<int32_t>(output.Width); pass.output_rect[3] = static_cast<int32_t>(output.Height);
        pass.camera_valid = decode_camera(value.constants, value.color.width, value.color.height,
            static_cast<uint32_t>(output.Width), output.Height, value.seconds, previous_jitter, pass.camera);
        std::memcpy(pass.previous_clip_to_clip, value.constants.previous_to_clip, sizeof(pass.previous_clip_to_clip));
        pass.motion_to_uv[0] = -value.constants.motion_scale[0]; pass.motion_to_uv[1] = -value.constants.motion_scale[1];
        pass.motion_camera_included = value.constants.camera_motion == 1;
        if (pass.camera_valid) {
            auto native = native_object(commands);
            ComPtr<ID3D12GraphicsCommandList> native_list;
            if (FAILED(native.As(&native_list))) return original_evaluate(feature, token, inputs, count, commands);
            auto* list = native_list.Get();
            Input* resources[] = {&value.color, &value.output, &value.depth, &value.motion, &value.exposure};
            static unsigned input_reports{};
            if (input_reports++ < 8) {
                for (size_t i = 0; i < std::size(resources); ++i) if (resources[i]->resource) {
                    const auto desc = resources[i]->resource->GetDesc();
                    char message[256]{};
                    std::snprintf(message, sizeof(message), "GTA native SR frame=%llu role=%zu resource=%p state=0x%x format=%u size=%llux%u list=%p.",
                        static_cast<unsigned long long>(id), i, static_cast<void*>(resources[i]->resource.Get()), resources[i]->state,
                        unsigned(desc.Format), static_cast<unsigned long long>(desc.Width), desc.Height, static_cast<void*>(list));
                    log(message);
                }
            }
            D3D12_RESOURCE_BARRIER barriers[5]{};
            for (size_t i = 0; i < std::size(resources); ++i) {
                barriers[i].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                barriers[i].Transition = {resources[i]->resource.Get(), D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                    static_cast<D3D12_RESOURCE_STATES>(resources[i]->state),
                    i == 1 ? D3D12_RESOURCE_STATE_COPY_DEST : D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE};
            }
            for (const auto& barrier : barriers) if (barrier.Transition.pResource && barrier.Transition.StateBefore != barrier.Transition.StateAfter) list->ResourceBarrier(1, &barrier);
            const auto result = host.process_upscale(host.user, list, &pass);
            for (auto& barrier : barriers) { const auto before = barrier.Transition.StateBefore; barrier.Transition.StateBefore = barrier.Transition.StateAfter; barrier.Transition.StateAfter = before; }
            for (const auto& barrier : barriers) if (barrier.Transition.pResource && barrier.Transition.StateBefore != barrier.Transition.StateAfter) list->ResourceBarrier(1, &barrier);
            previous_frame = id; std::memcpy(previous_jitter, value.constants.jitter, sizeof(previous_jitter));
            if (host.render_feedback) {
                const rsf_game_render_feedback feedback{sizeof(feedback), result ? RSF_GAME_RENDER_FEEDBACK_HOST_REFUSED : RSF_GAME_RENDER_FEEDBACK_ACCEPTED, host.session_id, id};
                host.render_feedback(host.user, &feedback);
            }
            if (!result) { native_gap.store(true); return 0; }
        }
    }
    previous_frame = 0;
    if (native_gap.load() && value.have_constants && count && inputs && inputs[0]) {
        auto reset = value.constants; reset.reset = 1;
        (void)original_constants(&reset, token, inputs[0]);
    }
    const auto result = original_evaluate(feature, token, inputs, count, commands);
    if (!result) native_gap.store(false);
    return result;
}
int fg_options(const void* view, const void* options)
{
    // RSF takes native FG policy ownership before replacing native SR, avoiding
    // a second interpolator consuming the game SR feature's stale inputs.
    std::array<unsigned char, 112> copy{};
    if (!active.load() || !read(options, copy.data(), copy.size())) return original_fg_options(view, options);
    const uint32_t off = 0; std::memcpy(copy.data() + 32, &off, sizeof(off));
    return original_fg_options(view, copy.data());
}
int feature_function(uint32_t feature, const char* name, void** output)
{
    const auto result = original_feature(feature, name, output);
    if (!result && active.load() && name && output && *output && feature == 1000 && !std::strcmp(name, "slDLSSGSetOptions")) {
        original_fg_options = reinterpret_cast<Options>(*output); *output = reinterpret_cast<void*>(&fg_options);
    }
    return result;
}
using Gpa = FARPROC (WINAPI *)(HMODULE, LPCSTR);
using GpaCaller = FARPROC (WINAPI *)(HMODULE, LPCSTR, void*);
Gpa original_gpa{};
GpaCaller original_gpa_caller{};
std::mutex binding_guard;
FARPROC intercept(HMODULE module, LPCSTR name, FARPROC result, void* caller)
{
    const auto address = reinterpret_cast<uintptr_t>(caller);
    if (!result || !active.load() || reinterpret_cast<uintptr_t>(name) <= 0xffff ||
        address < image || address >= image + 0x5b45200) return result;
    std::lock_guard lock(binding_guard);
#define BIND(export_name, original, replacement) if (!std::strcmp(name, export_name)) { \
    const auto wrapper = reinterpret_cast<FARPROC>(&replacement); \
    if (result == wrapper) return wrapper; \
    if (original && reinterpret_cast<FARPROC>(original) != result) return result; \
    original = reinterpret_cast<decltype(original)>(result); return wrapper; }
    BIND("slSetConstants", original_constants, constants)
    BIND("slSetD3DDevice", original_set_device, set_device)
    BIND("slSetTagForFrame", original_tags, tags)
    BIND("slEvaluateFeature", original_evaluate, evaluate)
    BIND("slGetNewFrameToken", original_token, new_token)
    BIND("slGetFeatureFunction", original_feature, feature_function)
#undef BIND
    if (!std::strcmp(name, "CreateDXGIFactory2") && !original_factory) {
        HMODULE owner{};
        if (GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                reinterpret_cast<const wchar_t*>(result), &owner) && owner == module) {
            if (MH_CreateHook(reinterpret_cast<void*>(result), reinterpret_cast<void*>(&create_factory),
                    reinterpret_cast<void**>(&original_factory)) == MH_OK) MH_EnableHook(reinterpret_cast<void*>(result));
        }
    }
    return result; // Preserve creation-export identity for Steam/RTSS installers.
}
FARPROC WINAPI get_proc(HMODULE module, LPCSTR name)
{ return intercept(module, name, original_gpa(module, name), _ReturnAddress()); }
FARPROC WINAPI get_proc_caller(HMODULE module, LPCSTR name, void* caller)
{ return intercept(module, name, original_gpa_caller(module, name, caller), caller); }
}

rsf_result rsf_gta5_renderer_prepare(const rsf_game_host_services& services) noexcept
{
    if (prepared) return RSF_ERROR_BUSY;
    if (!services.process_upscale || !services.submit || !services.present) return RSF_ERROR_NOT_READY;
    image = reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
    IMAGE_DOS_HEADER dos{}; IMAGE_NT_HEADERS64 nt{};
    if (!read(reinterpret_cast<void*>(image), &dos, sizeof(dos)) || dos.e_magic != IMAGE_DOS_SIGNATURE ||
        !read(reinterpret_cast<void*>(image + dos.e_lfanew), &nt, sizeof(nt)) || nt.Signature != IMAGE_NT_SIGNATURE ||
        nt.FileHeader.TimeDateStamp != 0x6aa45f10 || nt.OptionalHeader.SizeOfImage != 0x5b45200) return RSF_ERROR_NATIVE_REFUSED;
    const auto initialized = MH_Initialize();
    if (initialized != MH_OK && initialized != MH_ERROR_ALREADY_INITIALIZED) return RSF_ERROR_NATIVE_REFUSED;
    if (MH_CreateHookApi(L"kernelbase.dll", "GetProcAddress", reinterpret_cast<void*>(&get_proc),
            reinterpret_cast<void**>(&original_gpa)) != MH_OK ||
        MH_CreateHookApi(L"kernelbase.dll", "GetProcAddressForCaller", reinterpret_cast<void*>(&get_proc_caller),
            reinterpret_cast<void**>(&original_gpa_caller)) != MH_OK) return RSF_ERROR_NATIVE_REFUSED;
    host = services; QueryPerformanceFrequency(&frequency); prepared = true;
    if (HMODULE runtime = GetModuleHandleW(L"ReScaleFrame.Runtime.dll")) {
        attach_factory = reinterpret_cast<AttachGraphics>(GetProcAddress(runtime, "rsf_render_host_factory"));
        attach_device = reinterpret_cast<AttachGraphics>(GetProcAddress(runtime, "rsf_render_host_device"));
        native_object_fn = reinterpret_cast<NativeObject>(GetProcAddress(runtime, "rsf_render_host_native_object"));
        graphics_ready_fn = reinterpret_cast<GraphicsReady>(GetProcAddress(runtime, "rsf_render_host_graphics_ready"));
    }
    log("GTA native frame/resource interception prepared for the fingerprinted Enhanced build.");
    return RSF_OK;
}
rsf_result rsf_gta5_renderer_start() noexcept
{
    if (!prepared) return RSF_ERROR_NOT_READY;
    active.store(true);
    if (MH_EnableHook(MH_ALL_HOOKS) != MH_OK) { active.store(false); return RSF_ERROR_NATIVE_REFUSED; }
    return RSF_OK;
}
rsf_result rsf_gta5_renderer_quiesce() noexcept
{ active.store(false); return callbacks.load() ? RSF_ERROR_BUSY : RSF_OK; }
rsf_result rsf_gta5_renderer_stop() noexcept
{
    if (active.load() || callbacks.load()) return RSF_ERROR_BUSY;
    // Resolver thunks cached by the engine also pin this plugin until process exit.
    return prepared ? RSF_ERROR_BUSY : RSF_OK;
}
void rsf_gta5_renderer_status(rsf_game_renderer_status& output) noexcept
{
    output.prepared = prepared; output.active = active.load(); output.rendering_ready = 0;
    output.reason = "Native SR input interception and shared backend reinsertion; FG and runtime controls are being integrated.";
}

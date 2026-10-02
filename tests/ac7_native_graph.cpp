// SPDX-License-Identifier: GPL-3.0-only
// Exercise the private adapter through an independent engine ABI caller and queued executor.
#define RSF_AC7_GRAPH_TEST 1
#include "../games/ac7/src/native_renderer.cpp"
#include "ac7_view_fixture.h"
#include <rescaleframe/native_sr.h>
#include <wrl/client.h>
#include <cstdio>
using Microsoft::WRL::ComPtr;
namespace harness {
bool passed = true;
void check(bool ok, const char* why) { if (!ok) { passed = false; std::fprintf(stderr, "%s\n", why); } }
struct Command { Command* next = nullptr; void(*execute)(void*, Command*) = nullptr; };
struct List { Command* root = nullptr; Command** tail = &root; uint32_t executing = 0, count = 0, uid = 1; } list;
static_assert(offsetof(List, count) == 0x14);
struct Texture { void** table = nullptr; ID3D11Texture2D* native = nullptr; };
struct Pool { void** table = nullptr; Texture* target = nullptr; Texture* shader = nullptr; uint32_t refs = 1; };
uint32_t add(Pool* p) { return ++p->refs; }
uint32_t drop(Pool* p) { check(p->refs > 0, "native pool released twice"); return --p->refs; }
void* get_native(Texture* p) { return p->native; }
void* texture_table[7]{}; void* pool_table[7]{};
struct Pass { void** table = nullptr; uint64_t flags = 0; NativeRef ref; NativeOutput out; NativeRef auxiliary; };
void* get_input(Pass* p, uint32_t i) { return i == 0 ? &p->ref : i == 1 && p->auxiliary.node ? &p->auxiliary : nullptr; }
void* get_output(Pass* p, uint32_t i) { return i ? nullptr : &p->out; }
void* get_desc(Pass* p, void* out, uint32_t) {
    auto* input = node_output(p->ref.node, p->ref.index);
    std::memcpy(out, input ? input->descriptor : p->out.descriptor, 0x50); return out;
}
std::array<unsigned char, 0x280> scene{};
std::array<unsigned char, 0x60> context{};
std::array<unsigned char, 0x27c0> view{};
ComPtr<ID3D11Device> device; ComPtr<ID3D11DeviceContext> immediate;
ComPtr<ID3D11RenderTargetView> target_view;
Pool* output_pool_ptr = nullptr;
SRNode* registered = nullptr;
uint32_t fallback_draws = 0, sr_calls = 0, aa_draws = 0, uniform_rebuilds = 0, family_rebuilds = 0;
uint32_t configured_output = 4;
uint32_t configured_width = 2, configured_height = 2;
struct Uniform { uint64_t unused = 0; LONG refs = 1; uint32_t padding = 0; };
Uniform original_uniform, generated_uniform;
uint64_t observed_frame = 0; bool auxiliary_done = false;
struct Clear : Command { float color[4]{}; };
void clear(void*, Command* c) { auto* cmd = static_cast<Clear*>(c); immediate->ClearRenderTargetView(target_view.Get(), cmd->color); delete cmd; }
void spatial(void* raw_node, void* raw_context) {
    check(auxiliary_done, "native auxiliary graph branches finish before SR size transition");
    ++fallback_draws; auto* node = static_cast<SRNode*>(raw_node);
    int32_t current[4]{}; copy(current, view.data() + 0x70, 16);
    check(current[2] == 2 && current[3] == 2, "fallback must read original source rectangle");
    check(node->output.descriptor[0x2c] == 10, "SR graph keeps linear floating point output");
    check(raw_context == context.data(), "native context forwarded");
    node->output.pool = uint64_t(uintptr_t(output_pool_ptr));
    auto* c = new Clear; c->color[2] = c->color[3] = 1; c->execute = clear;
    *list.tail = c; list.tail = &c->next; ++list.count;
}
void tone(void* raw, void*) {
    auto* pass = static_cast<Pass*>(raw);
    pass->out.pool = output_pool(pass->ref.node, pass->ref.index);
    pool_ref(pass->out.pool, 0x28);
}
void aa(void*, void*) { ++aa_draws; }
void material(void*, void*) {}
void source_process(void*, void*) {}
void auxiliary_process(void*, void*) {
    int32_t rect[4]{}; copy(rect, view.data()+0x70, 16);
    check(rect[2] == 2 && rect[3] == 2, "bloom/grading producer keeps original native rectangle");
    auxiliary_done = true;
}
void hud(void*, void*) {}
void composite(void*, void*) {}
void* get_scene() { return scene.data(); }
void* register_node(void* graph, void* node) {
    check(graph == context.data() + 0x18, "registration uses context graph owner");
    check(registered == nullptr, "one SR node per graph"); registered = static_cast<SRNode*>(node); return node;
}
void remove_jitter(void* matrices) { std::memset(static_cast<unsigned char*>(matrices) + 0x360, 0, 8); }
void parameters(void*, void*, void*, void*, void*, uint32_t count, void*) {
    check(count == 2, "native uniform builder gets both lighting-volume bounds"); ++uniform_rebuilds;
}
void* create_uniform(void*, uint64_t* out, const void*, const void*, uint32_t usage) {
    check(usage == 1, "postprocess uniform is single-frame engine allocation");
    *out = uint64_t(uintptr_t(&generated_uniform)); return out;
}
void* move_uniform(void* dst, void* src) {
    uint64_t previous = 0; std::memcpy(&previous, dst, 8);
    if (previous) InterlockedDecrement(&reinterpret_cast<Uniform*>(uintptr_t(previous))->refs);
    std::memcpy(dst, src, 8); std::memset(src, 0, 8); return dst;
}
void wait_recorders() {}
void unmatched_downsample() {}
void* dynamic_table[31]{};
void** dynamic_object = dynamic_table;
void* dynamic_pointer = &dynamic_object;
uint64_t layout = 0;
void family_size(void*) { ++family_rebuilds; }
void visibility(void* raw, void*, uintptr_t third, void*) {
    uint64_t storage = 0; read(uint64_t(uintptr_t(raw)), 0xb8, storage);
    uint32_t first = 0, second = 0;
    read(storage, 0x13c0, first); read(storage + 0x27c0, 0x13c0, second);
    check(first == 2 && second == 1 && third == 77, "native jitter preparation selects main view and forwards engine arguments");
}
void* resolve_engine(uint32_t rva) {
    switch (rva) {
    case 0x109dc70: return reinterpret_cast<void*>(&get_scene);
    case 0xe93670: return reinterpret_cast<void*>(&register_node);
    case 0xff9af0: return reinterpret_cast<void*>(&remove_jitter);
    case 0x11346d0: return reinterpret_cast<void*>(&parameters);
    case 0xde5cf0: return reinterpret_cast<void*>(&move_uniform);
    case 0x3c783d0: return &dynamic_pointer;
    case 0x3caa6c8: return &layout;
    case 0x19b6fe0: return reinterpret_cast<void*>(&family_size);
    case 0x12183c0: return reinterpret_cast<void*>(&wait_recorders);
    case 0xfb7a00: return reinterpret_cast<void*>(&unmatched_downsample);
    default: check(false, "unexpected native helper in adapter"); return nullptr;
    }
}
int config(void*, rsf_game_render_config* c) { c->enabled = 1; c->output_width = c->output_height = configured_output; c->render_width = configured_width; c->render_height = configured_height; return 1; }
void notify(void*, void*, const rsf_ac7_render_scope* p, uint32_t begin) {
    if (!begin || p->role != RSF_AC7_ROLE_SR) return;
    ++sr_calls; observed_frame = p->native_frame;
    check(p->color_output == output_pool_ptr->target->native && p->depth && p->motion, "resolved leased resources are current");
    check(p->camera.render_width == 2 && p->camera.output_width == 4, "camera keeps pre-SR units");
    check(rsf_native_sr_evaluate(immediate.Get(), p) == RSF_DLSS_PIPELINE_ERROR_NOT_RUNNING,
        "a stopped backend refuses without touching the native spatial fallback");
}
// The caller gathers child outputs first, writes each returned descriptor and runs native nodes.
void gather(uint64_t node) {
    const auto dependency = method(node, 0x40);
    for (uint32_t i = 0;; ++i) {
        auto* dep = static_cast<NativeRef*>(reinterpret_cast<GetPointer>(uintptr_t(dependency))(reinterpret_cast<void*>(uintptr_t(node)), i));
        if (!dep) break; if (dep->node) gather(dep->node);
    }
    auto* out = node_output(node); unsigned char desc[0x50]{};
    reinterpret_cast<void*(*)(void*, void*, uint32_t)>(uintptr_t(method(node, 0x70)))(reinterpret_cast<void*>(uintptr_t(node)), desc, 0);
    std::memcpy(out->descriptor, desc, sizeof(desc));
}
void run() {
    list.executing = 1;
    for (auto* cmd = list.root; cmd;) { auto* next = cmd->next; cmd->execute(&list, cmd); cmd = next; }
    list.executing = 0; list.root = nullptr; list.tail = &list.root; list.count = 0;
}
void execute_graph(uint64_t node, void* ctx) {
    auto* bytes = reinterpret_cast<unsigned char*>(uintptr_t(node)); if (bytes[9]) return; bytes[9] = 1;
    const auto dependency = method(node, 0x40);
    for (uint32_t i = 0;; ++i) {
        auto* ref = static_cast<NativeRef*>(reinterpret_cast<GetPointer>(uintptr_t(dependency))(reinterpret_cast<void*>(uintptr_t(node)), i));
        if (!ref) break; if (ref->node) execute_graph(ref->node, ctx);
    }
    reinterpret_cast<Process>(uintptr_t(method(node, 0x28)))(reinterpret_cast<void*>(uintptr_t(node)), ctx);
}
void original_context(void* raw_context, void* raw_root) {
    gather(uint64_t(uintptr_t(raw_root))); execute_graph(uint64_t(uintptr_t(raw_root)), raw_context);
    int32_t rect[4]{}; copy(rect, view.data() + 0x70, 16);
    check(rect[2] == 4 && rect[3] == 4, "post-SR consumers get output rectangle in same graph");
    int32_t size[2]{}; copy(size, scene.data() + 0x208, 8);
    check(size[0] == 4 && size[1] == 4, "post-SR shader source size is native output size");
    check(aa_draws == 0, "FXAA does not filter the temporal reconstruction again");
}
}
int main() {
    using namespace harness;
    if (FAILED(D3D11CreateDevice(nullptr, D3D_DRIVER_TYPE_WARP, nullptr, 0, nullptr, 0, D3D11_SDK_VERSION,
        &device, nullptr, &immediate))) return 77;
    D3D11_TEXTURE2D_DESC desc{}; desc.Width = desc.Height = 4; desc.MipLevels = desc.ArraySize = 1;
    desc.Format = DXGI_FORMAT_R16G16B16A16_FLOAT; desc.SampleDesc.Count = 1;
    desc.BindFlags = D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE;
    ComPtr<ID3D11Texture2D> color;
    check(SUCCEEDED(device->CreateTexture2D(&desc, nullptr, &color)), "create native output");
    check(SUCCEEDED(device->CreateRenderTargetView(color.Get(), nullptr, &target_view)), "create native fallback target");
    texture_table[6] = reinterpret_cast<void*>(&get_native);
    pool_table[5] = reinterpret_cast<void*>(&add); pool_table[6] = reinterpret_cast<void*>(&drop);
    Texture texture{texture_table, color.Get()}; Pool input{pool_table, &texture, &texture}, depth{pool_table, &texture, &texture}, output{pool_table, &texture, &texture};
    output_pool_ptr = &output;
    rsf_ac7_native_renderer renderer; renderer.options.struct_size = sizeof(renderer.options);
    renderer.options.session_id = 7; renderer.options.render_config = config;
    renderer.render_thread.store(GetCurrentThreadId()); renderer.active.store(true);
    check(rsf_ac7_render_scopes_create(7, 32, notify, nullptr, &renderer.scopes) != 0, "create execution scopes");
    installed.store(&renderer); test_engine = resolve_engine;
    dynamic_table[30] = reinterpret_cast<void*>(&create_uniform);
    Process originals[] = {tone, aa, material, hud, composite, spatial};
    void* wrappers[]{reinterpret_cast<void*>(&hooked_process<0>), reinterpret_cast<void*>(&hooked_process<1>),
        reinterpret_cast<void*>(&hooked_process<2>), reinterpret_cast<void*>(&hooked_process<3>),
        reinterpret_cast<void*>(&hooked_process<4>), reinterpret_cast<void*>(&hooked_process<5>)};
    for (uint32_t i = 0; i < 6; ++i) { sites[i].target = wrappers[i]; sites[i].original = reinterpret_cast<void*>(originals[i]); sites[i+6].original = reinterpret_cast<void*>(&get_desc); }
    sites[13].original = reinterpret_cast<void*>(&original_context);
    void* source_table[17]{}; void* tone_table[17]{}; void* aa_table[17]{}; void* auxiliary_table[17]{};
    for (auto* table : {source_table, tone_table, aa_table, auxiliary_table}) {
        table[1] = reinterpret_cast<void*>(&get_input); table[2] = table[1];
        table[8] = reinterpret_cast<void*>(&get_input); table[7] = reinterpret_cast<void*>(&get_output); table[14] = reinterpret_cast<void*>(&get_desc);
    }
    tone_table[5] = wrappers[0]; aa_table[5] = wrappers[1];
    source_table[5] = reinterpret_cast<void*>(&source_process); auxiliary_table[5] = reinterpret_cast<void*>(&auxiliary_process);
    Pass source, tonemap, anti_alias, auxiliary; auxiliary.table = auxiliary_table; source.table = source_table; tonemap.table = tone_table; anti_alias.table = aa_table;
    source.out.pool = uint64_t(uintptr_t(&input)); int32_t internal[2]{2,2}; std::memcpy(source.out.descriptor+0x14, internal, 8);
    auxiliary.out = source.out; tonemap.auxiliary.node = uint64_t(uintptr_t(&auxiliary));
    tonemap.ref.node = uint64_t(uintptr_t(&source)); anti_alias.ref.node = uint64_t(uintptr_t(&tonemap));
    auto fixture = ac7_fixture::make_view(1, 1, 0.1f, 2, 2, 2, 2);
    fixture.put(ac7_fixture::kViewToClip+32, {0.125f, -0.25f});
    fixture.put(ac7_fixture::kTemporalAAJitter, {0.125f, -0.25f, 0, 0});
    // Complete inverse of the fixture's jittered projection.
    fixture.put(ac7_fixture::kClipToView+48, {-0.125f, 0.25f});
    uint64_t cached = uint64_t(uintptr_t(fixture.values.data())), family = uint64_t(uintptr_t(scene.data()));
    const uint32_t frame = 41; uint64_t state = 0x123456;
    uint64_t old_uniform = uint64_t(uintptr_t(&original_uniform)); std::memcpy(view.data()+0x10, &old_uniform, 8);
    std::memcpy(view.data(), &family, 8); std::memcpy(view.data()+8, &state, 8);
    int32_t render[4]{0,0,2,2}, full[4]{0,0,4,4};
    std::memcpy(view.data()+0x70, render, 16); std::memcpy(view.data()+0x80, full, 16);
    std::memcpy(view.data()+0x90, full, 16);
    std::memcpy(view.data()+0x1418, &cached, 8); std::memcpy(scene.data()+0x68, &frame, 4);
    auto pool_address = uint64_t(uintptr_t(&depth)); std::memcpy(scene.data()+0x60, &pool_address, 8); std::memcpy(scene.data()+0x208, internal, 8);
    uint64_t view_address = uint64_t(uintptr_t(view.data())), list_address = uint64_t(uintptr_t(&list));
    std::memcpy(context.data(), &view_address, 8); std::memcpy(context.data()+0x28, &list_address, 8);
    postprocess_view = view_address; postprocess_velocity = uint64_t(uintptr_t(&input));
    hooked_context(context.data(), &anti_alias);
    check(registered != nullptr && fallback_draws == 1, "graph has native SR node and complete spatial fallback");
    check(std::memcmp(view.data()+0x70, render, 16) == 0 && std::memcmp(scene.data()+0x208, internal, 8) == 0,
        "CPU view/scene sizes restored after graph construction");
    uint64_t restored_uniform = 0; copy(&restored_uniform, view.data()+0x10, 8);
    check(uniform_rebuilds == 1 && restored_uniform == old_uniform,
        "only shader parameters rebuilt; original native uniform ownership restored");
    if (!registered) return 1;
    // Native graph retirement happens before its queued RHI work runs.
    node_release(registered); registered = nullptr;
    check(output.refs >= 2, "queued pool leases survive native graph retirement");
    const uint32_t later = 900; std::memcpy(scene.data()+0x68, &later, 4);
    run(); drain_retired(renderer);
    D3D11_TEXTURE2D_DESC read_desc = desc; read_desc.BindFlags = 0; read_desc.Usage = D3D11_USAGE_STAGING;
    read_desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
    ComPtr<ID3D11Texture2D> readback;
    check(SUCCEEDED(device->CreateTexture2D(&read_desc, nullptr, &readback)), "create fallback readback");
    immediate->CopyResource(readback.Get(), color.Get()); D3D11_MAPPED_SUBRESOURCE mapped{};
    if (SUCCEEDED(immediate->Map(readback.Get(), 0, D3D11_MAP_READ, 0, &mapped))) {
        const auto* pixel = static_cast<const uint16_t*>(mapped.pData);
        check(pixel[0] == 0 && pixel[1] == 0 && pixel[2] == 0x3c00 && pixel[3] == 0x3c00,
            "refused SR preserves the native blue fallback pixels");
        immediate->Unmap(readback.Get(), 0);
    } else check(false, "read fallback pixels");
    pool_ref(tonemap.out.pool, 0x30); pool_ref(anti_alias.out.pool, 0x30);
    check(sr_calls == 1 && observed_frame == 41, "delayed RHI execution keeps original frame identity");
    check(input.refs == 1 && depth.refs == 1, "lease refs retired on original render owner");
    GraphPlan refused;
    check(!make_plan(renderer, context.data(), &source, refused), "unknown graph path refuses without guessing texture roles");
    std::array<unsigned char, 0x220> owned_renderer{};
    std::array<unsigned char, 0x27c0 * 2> owned_views{};
    uint64_t owned_address = uint64_t(uintptr_t(owned_views.data())); int32_t owned_count = 2;
    std::memcpy(owned_renderer.data()+0xb8, &owned_address, 8); std::memcpy(owned_renderer.data()+0xc0, &owned_count, 4);
    int32_t primary[4]{0,0,16,16}, secondary[4]{0,0,8,8}; uint32_t mode = 1;
    std::memcpy(owned_views.data()+0x80, primary, 16); std::memcpy(owned_views.data()+0x27c0+0x80, secondary, 16);
    std::memcpy(owned_views.data()+0x70, primary, 16); std::memcpy(owned_views.data()+0x27c0+0x70, secondary, 16);
    for (size_t offset : {size_t(0), size_t(0x27c0)}) {
        std::memcpy(owned_views.data()+offset+8, &state, 8);
        std::memcpy(owned_views.data()+offset+0x13c0, &mode, 4);
        std::memcpy(owned_views.data()+offset+0x90, owned_views.data()+offset+0x80, 16);
    }
    rsf_game_render_config desired{sizeof(desired),1,16,16,7,5};
    prepare_owned_views(owned_renderer.data(), desired);
    int32_t adjusted[4]{}; copy(adjusted, owned_views.data()+0x70, 16);
    check(adjusted[2] == 7 && adjusted[3] == 5 && family_rebuilds == 1,
        "renderer producer keeps exact backend rectangle before recomputing family allocation extent");
    check(std::memcmp(owned_views.data()+0x27c0+0x70, secondary, 16) == 0,
        "secondary view retains its producer resolution");
    int32_t native_buffer[2]{8,8}; std::memcpy(scene.data()+0x208, native_buffer, 8);
    configured_output = 16; configured_width = 7; configured_height = 5;
    sites[14].original = reinterpret_cast<void*>(&visibility);
    hooked_visibility(owned_renderer.data(), nullptr, 77, nullptr);
    uint32_t restored_mode = 0; copy(&restored_mode, owned_views.data()+0x13c0, 4);
    check(restored_mode == 1, "native AA graph choice restored after jitter producer");
    configured_output = 4; configured_width = configured_height = 2;
    rsf_ac7_render_scopes_quiesce(renderer.scopes);
    check(rsf_ac7_render_scopes_destroy(renderer.scopes) != 0, "quiescent graph unload"); installed.store(nullptr);
    return passed ? 0 : 1;
}

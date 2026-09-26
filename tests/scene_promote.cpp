// Hand the promotion a frame's tail and check the plan it builds.
//
// The textures here stand in for the game's: a render resolution composite, two render resolution
// interface layers, one chain intermediate, the scene colour, and an output resolution
// reconstruction. What is checked is the plan, because the plan is the whole of what crosses into
// the frame tap and every mistake in it is a mistake in what the game draws.
//
// What cannot be checked here is whether the substitution produces a correct picture. That needs
// the game's own shaders reading the game's own constants, and it is stated as unverified in
// scene_promote.h rather than implied by a passing test.

#include <rescaleframe/frame_tap.h>
#include <rescaleframe/scene_promote.h>

#include <windows.h>

#include <d3d11.h>

#include <cstdio>
#include <string>
#include <vector>

namespace {

bool passed = true;

void check(bool condition, const char* message)
{
    if (!condition) {
        std::fprintf(stderr, "%s\n", message);
        passed = false;
    }
}

void stage(const char* what)
{
    std::fprintf(stderr, "[stage] %s\n", what);
    std::fflush(stderr);
}

std::vector<std::string> log_lines;

void collect(void* user, const char* message)
{
    (void)user;
    log_lines.emplace_back(message);
}

bool logged(const char* fragment)
{
    for (const std::string& line : log_lines) {
        if (line.find(fragment) != std::string::npos) {
            return true;
        }
    }
    return false;
}

ID3D11Texture2D* make_target(ID3D11Device* device, UINT width, UINT height, DXGI_FORMAT format)
{
    D3D11_TEXTURE2D_DESC description{};
    description.Width = width;
    description.Height = height;
    description.MipLevels = 1;
    description.ArraySize = 1;
    description.Format = format;
    description.SampleDesc.Count = 1;
    description.Usage = D3D11_USAGE_DEFAULT;
    description.BindFlags = D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE;
    ID3D11Texture2D* texture = nullptr;
    if (FAILED(device->CreateTexture2D(&description, nullptr, &texture))) {
        return nullptr;
    }
    return texture;
}

// The plan entry naming a texture, or null. The order entries land in is an implementation detail
// and searching by texture is what a reader of the plan would do.
const rsf_frame_tap_substitution* entry_for(const rsf_frame_tap_plan& plan, void* texture)
{
    for (uint32_t index = 0; index < plan.count; ++index) {
        if (plan.items[index].texture == texture) {
            return &plan.items[index];
        }
    }
    return nullptr;
}

// A promoted surface is both written and read at output resolution, so its entry carries both
// views and is not gated.
void check_promoted(const rsf_frame_tap_plan& plan, void* texture, const char* what)
{
    const rsf_frame_tap_substitution* item = entry_for(plan, texture);
    std::string missing = std::string("The plan must name the ") + what + ".";
    check(item != nullptr, missing.c_str());
    if (!item) {
        return;
    }
    std::string views = std::string("The ") + what + " is written and read, so it needs both views.";
    check(item->render_view != nullptr && item->shader_view != nullptr, views.c_str());
    std::string gate = std::string("The ") + what +
                       " must be substituted from the start of the frame, not gated.";
    check(item->after_target == nullptr, gate.c_str());
}

} // namespace

int main()
{
    stage("validating arguments");
    rsf_promote_setup setup{};
    setup.struct_size = sizeof(setup);
    setup.abi_version = RSF_PROMOTE_ABI_VERSION;
    setup.output_width = 512;
    setup.output_height = 288;
    setup.log = collect;

    rsf_promote* promote = nullptr;
    check(rsf_promote_create(&setup, &promote) == RSF_PROMOTE_ERROR_INVALID_ARGUMENT,
          "Creating without a device must be refused.");

    stage("creating device");
    ID3D11Device* device = nullptr;
    ID3D11DeviceContext* context = nullptr;
    D3D_FEATURE_LEVEL level{};
    if (FAILED(D3D11CreateDevice(nullptr, D3D_DRIVER_TYPE_HARDWARE, nullptr, 0, nullptr, 0,
                                 D3D11_SDK_VERSION, &device, &level, &context)) ||
        !device || !context) {
        std::fprintf(stderr, "no D3D11 device available, skipping\n");
        return 0;
    }
    setup.device = device;

    setup.abi_version = RSF_PROMOTE_ABI_VERSION + 1u;
    check(rsf_promote_create(&setup, &promote) == RSF_PROMOTE_ERROR_ABI_MISMATCH,
          "An incompatible ABI must be rejected.");
    setup.abi_version = RSF_PROMOTE_ABI_VERSION;
    check(rsf_promote_create(&setup, &promote) == RSF_PROMOTE_OK && promote,
          "Creating must succeed.");

    stage("creating a frame's tail");
    // Half the output in each direction, which is the render scale the loader sets. Two interface
    // layers because AC7 alternates between two allocations from frame to frame.
    ID3D11Texture2D* composite = make_target(device, 256, 144, DXGI_FORMAT_B8G8R8A8_UNORM);
    ID3D11Texture2D* layer_a = make_target(device, 256, 144, DXGI_FORMAT_R8G8B8A8_UNORM);
    ID3D11Texture2D* layer_b = make_target(device, 256, 144, DXGI_FORMAT_R8G8B8A8_UNORM);
    ID3D11Texture2D* chain = make_target(device, 256, 144, DXGI_FORMAT_B8G8R8A8_UNORM);
    ID3D11Texture2D* scene_color = make_target(device, 256, 144, DXGI_FORMAT_R16G16B16A16_FLOAT);
    ID3D11Texture2D* reconstruction = make_target(device, 512, 288, DXGI_FORMAT_R16G16B16A16_FLOAT);
    check(composite && layer_a && layer_b && chain && scene_color && reconstruction,
          "The stand-in textures must be created.");
    if (!composite || !layer_a || !layer_b || !chain || !scene_color || !reconstruction) {
        return 1;
    }

    rsf_promote_frame_tail tail{};
    tail.struct_size = sizeof(tail);
    tail.composite = composite;
    tail.ui_targets[0] = layer_a;
    tail.ui_targets[1] = layer_b;
    tail.ui_target_count = 2;
    tail.chain_targets[0] = chain;
    tail.chain_target_count = 1;
    tail.scene_color = scene_color;
    tail.reconstruction = reconstruction;
    tail.render_width = 256;
    tail.render_height = 144;

    stage("refusing a frame with nothing to upscale");
    rsf_promote_frame_tail unscaled = tail;
    unscaled.render_width = 512;
    unscaled.render_height = 288;
    check(rsf_promote_prepare(promote, &unscaled) == RSF_PROMOTE_ERROR_NOT_SCALED,
          "A frame already at output resolution must be refused rather than substituted into.");
    check(logged("nothing to promote"),
          "Refusing must say why, since it is the same thing a mission load causes.");

    stage("preparing");
    check(rsf_promote_prepare(promote, &tail) == RSF_PROMOTE_OK, "Preparing must succeed.");

    rsf_promote_status status{};
    status.struct_size = sizeof(status);
    check(rsf_promote_get_status(promote, &status) == RSF_PROMOTE_OK, "Status must be readable.");
    check(status.ready == 1, "Status must say it is ready.");
    check(status.ui_targets_promoted == 2, "Both interface layers must be reported as promoted.");
    check(status.chain_targets_promoted == 1, "The chain target must be reported as promoted.");
    check(status.render_width == 256 && status.output_width == 512,
          "Status must carry both resolutions, which is what the scale is read from.");

    stage("building the plan");
    rsf_frame_tap_plan plan{};
    plan.struct_size = sizeof(plan);
    check(rsf_promote_fill_plan(promote, &plan) == RSF_PROMOTE_OK, "Filling the plan must succeed.");
    check(plan.count == 5,
          "The plan must cover the composite, two interface layers, the chain and scene colour.");
    check(plan.viewport_scale_x == 2.0f && plan.viewport_scale_y == 2.0f,
          "The viewport scale must be the ratio of the two resolutions.");
    check(plan.items[0].texture == composite,
          "The composite comes first: it is the gate scene colour waits on.");
    check(plan.items[plan.count - 1].texture == scene_color,
          "Scene colour comes last, after everything it is gated behind.");

    check_promoted(plan, composite, "composite");
    check_promoted(plan, layer_a, "first interface layer");
    check_promoted(plan, layer_b, "second interface layer");
    check_promoted(plan, chain, "chain target");

    const rsf_frame_tap_substitution* scene_item = entry_for(plan, scene_color);
    check(scene_item != nullptr, "The plan must name scene colour.");
    if (scene_item) {
        check(scene_item->shader_view != nullptr && scene_item->render_view == nullptr,
              "Scene colour is read and never written by the substitution: replacing it as a "
              "render target would redirect the passes that draw the scene.");
        check(scene_item->after_target == composite,
              "Scene colour must be gated on the composite, or the passes still drawing the scene "
              "would read a reconstruction of the frame they have not finished.");
    }

    stage("a layer offered twice is promoted once");
    rsf_promote_frame_tail doubled = tail;
    doubled.ui_targets[2] = layer_a;
    doubled.ui_target_count = 3;
    check(rsf_promote_prepare(promote, &doubled) == RSF_PROMOTE_OK,
          "A duplicate in the set must not fail the whole tail.");
    check(rsf_promote_get_status(promote, &status) == RSF_PROMOTE_OK &&
              status.ui_targets_promoted == 2,
          "A layer named twice is one replacement, not two entries fighting over one texture.");

    stage("preparing again without interface layers or a chain");
    log_lines.clear();
    rsf_promote_frame_tail bare = tail;
    bare.ui_target_count = 0;
    bare.chain_target_count = 0;
    check(rsf_promote_prepare(promote, &bare) == RSF_PROMOTE_OK,
          "An unidentified interface must still allow the scene to be promoted.");
    check(logged("magnified with the scene"),
          "An unpromoted interface must be said out loud, not left to be noticed in the picture.");
    check(logged("no chain target"),
          "A missing chain must be said out loud, since it is what made an earlier run cleaner "
          "but not sharper.");
    check(rsf_promote_get_status(promote, &status) == RSF_PROMOTE_OK &&
              status.ui_targets_promoted == 0 && status.chain_targets_promoted == 0,
          "Status must say nothing beyond the composite was promoted.");

    rsf_frame_tap_plan smaller{};
    smaller.struct_size = sizeof(smaller);
    check(rsf_promote_fill_plan(promote, &smaller) == RSF_PROMOTE_OK,
          "A plan without interface layers must still build.");
    check(smaller.count == 2, "That plan must cover the composite and scene colour only.");
    check(entry_for(smaller, layer_a) == nullptr && entry_for(smaller, chain) == nullptr,
          "It must not name a texture that was not identified.");

    stage("releasing");
    rsf_promote_destroy(promote);
    reconstruction->Release();
    scene_color->Release();
    chain->Release();
    layer_b->Release();
    layer_a->Release();
    composite->Release();
    context->Release();
    device->Release();

    std::fprintf(stderr, passed ? "scene_promote: pass\n" : "scene_promote: fail\n");
    return passed ? 0 : 1;
}

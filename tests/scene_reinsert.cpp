// Hand the reinsertion a frame's tail and check the plan it builds.
//
// The textures here stand in for the game's: a render resolution composite, a render resolution
// interface target, the scene colour, and an output resolution reconstruction. What is checked is
// the plan, because the plan is the whole of what crosses into the frame tap and every mistake in
// it is a mistake in what the game draws.
//
// What cannot be checked here is whether the substitution produces a correct picture. That needs
// the game's own shaders reading the game's own constants, and it is stated as unverified in
// scene_reinsert.h rather than implied by a passing test.

#include <rescaleframe/frame_tap.h>
#include <rescaleframe/scene_reinsert.h>

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

} // namespace

int main()
{
    stage("validating arguments");
    rsf_reinsert_setup setup{};
    setup.struct_size = sizeof(setup);
    setup.abi_version = RSF_REINSERT_ABI_VERSION;
    setup.output_width = 512;
    setup.output_height = 288;
    setup.log = collect;

    rsf_reinsert* reinsert = nullptr;
    check(rsf_reinsert_create(&setup, &reinsert) == RSF_REINSERT_ERROR_INVALID_ARGUMENT,
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

    setup.abi_version = RSF_REINSERT_ABI_VERSION + 1u;
    check(rsf_reinsert_create(&setup, &reinsert) == RSF_REINSERT_ERROR_ABI_MISMATCH,
          "An incompatible ABI must be rejected.");
    setup.abi_version = RSF_REINSERT_ABI_VERSION;
    check(rsf_reinsert_create(&setup, &reinsert) == RSF_REINSERT_OK && reinsert,
          "Creating must succeed.");

    stage("creating a frame's tail");
    // Half the output in each direction, which is the render scale the loader sets.
    ID3D11Texture2D* composite = make_target(device, 256, 144, DXGI_FORMAT_B8G8R8A8_UNORM);
    ID3D11Texture2D* interface_target = make_target(device, 256, 144, DXGI_FORMAT_R8G8B8A8_UNORM);
    ID3D11Texture2D* scene_color = make_target(device, 256, 144, DXGI_FORMAT_R16G16B16A16_FLOAT);
    ID3D11Texture2D* reconstruction = make_target(device, 512, 288, DXGI_FORMAT_R16G16B16A16_FLOAT);
    check(composite && interface_target && scene_color && reconstruction,
          "The stand-in textures must be created.");
    if (!composite || !interface_target || !scene_color || !reconstruction) {
        return 1;
    }

    rsf_reinsert_frame_tail tail{};
    tail.struct_size = sizeof(tail);
    tail.composite = composite;
    tail.interface_targets[0] = interface_target;
    tail.interface_target_count = 1;
    tail.scene_color = scene_color;
    tail.reconstruction = reconstruction;
    tail.render_width = 256;
    tail.render_height = 144;

    stage("refusing a frame with nothing to upscale");
    rsf_reinsert_frame_tail unscaled = tail;
    unscaled.render_width = 512;
    unscaled.render_height = 288;
    check(rsf_reinsert_prepare(reinsert, &unscaled) == RSF_REINSERT_ERROR_NOT_SCALED,
          "A frame already at output resolution must be refused rather than substituted into.");
    check(logged("nothing to reinsert into"),
          "Refusing must say why, since it is the same thing a mission load causes.");

    stage("preparing");
    check(rsf_reinsert_prepare(reinsert, &tail) == RSF_REINSERT_OK, "Preparing must succeed.");

    rsf_reinsert_status status{};
    status.struct_size = sizeof(status);
    check(rsf_reinsert_get_status(reinsert, &status) == RSF_REINSERT_OK, "Status must be readable.");
    check(status.ready == 1, "Status must say it is ready.");
    check(status.interface_promoted == 1,
          "An identified interface target must be reported as promoted.");
    check(status.render_width == 256 && status.output_width == 512,
          "Status must carry both resolutions, which is what the scale is read from.");

    stage("building the plan");
    rsf_frame_tap_plan plan{};
    plan.struct_size = sizeof(plan);
    check(rsf_reinsert_fill_plan(reinsert, &plan) == RSF_REINSERT_OK, "Filling the plan must succeed.");
    check(plan.count == 3, "The plan must cover the composite, the interface and scene colour.");
    check(plan.viewport_scale_x == 2.0f && plan.viewport_scale_y == 2.0f,
          "The viewport scale must be the ratio of the two resolutions.");

    const rsf_frame_tap_substitution* composite_item = entry_for(plan, composite);
    check(composite_item != nullptr, "The plan must name the composite.");
    if (composite_item) {
        check(composite_item->render_view != nullptr && composite_item->shader_view != nullptr,
              "The composite is both written and read, so it needs both views.");
        check(composite_item->after_target == nullptr,
              "The composite must be substituted from the start of the frame: the interface is "
              "drawn into it before the tonemap runs.");
    }

    const rsf_frame_tap_substitution* interface_item = entry_for(plan, interface_target);
    check(interface_item != nullptr, "The plan must name the interface target.");
    if (interface_item) {
        check(interface_item->render_view != nullptr,
              "The interface has to be drawn at output resolution, which needs its target "
              "replaced.");
    }

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

    stage("preparing again without an interface target");
    log_lines.clear();
    rsf_reinsert_frame_tail no_interface = tail;
    no_interface.interface_targets[0] = nullptr;
    no_interface.interface_target_count = 0;
    check(rsf_reinsert_prepare(reinsert, &no_interface) == RSF_REINSERT_OK,
          "An unidentified interface target must still allow the scene to be reinserted.");
    check(logged("magnified with the scene"),
          "An unpromoted interface must be said out loud, not left to be noticed in the picture.");
    check(rsf_reinsert_get_status(reinsert, &status) == RSF_REINSERT_OK &&
              status.interface_promoted == 0,
          "Status must say the interface was not promoted.");

    rsf_frame_tap_plan smaller{};
    smaller.struct_size = sizeof(smaller);
    check(rsf_reinsert_fill_plan(reinsert, &smaller) == RSF_REINSERT_OK,
          "A plan without an interface target must still build.");
    check(smaller.count == 2, "That plan must cover the composite and scene colour only.");
    check(entry_for(smaller, interface_target) == nullptr,
          "It must not name a texture that was not identified.");

    stage("releasing");
    rsf_reinsert_destroy(reinsert);
    reconstruction->Release();
    scene_color->Release();
    interface_target->Release();
    composite->Release();
    context->Release();
    device->Release();

    std::fprintf(stderr, passed ? "scene_reinsert: pass\n" : "scene_reinsert: fail\n");
    return passed ? 0 : 1;
}

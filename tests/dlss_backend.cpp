// The DLSS contract, checked without an NVIDIA GPU in the loop.
//
// What is testable here is the part that decides whether DLSS runs at all: argument and ABI
// rejection, the ordering rules, and that a missing or unloadable interposer is reported as such
// rather than crashing a game process. Everything past slInit needs a driver and a real device.
//
// Set RSF_STREAMLINE_BIN to the directory holding sl.interposer.dll and this test additionally
// performs a real load and asks the driver whether DLSS is supported. That path is skipped, not
// failed, when the variable is unset or the load does not succeed, since a machine without the
// SDK deployed is not a broken one.
//
// Under Wine that run needs DXVK, vkd3d-proton and DXVK-NVAPI selected together:
//
//   WINEDLLOVERRIDES="d3d11,d3d12,d3d12core,dxgi,nvapi,nvapi64,nvofapi64,nvngx,_nvngx=n"
//
// d3d12 belongs in that list even though nothing here wants D3D12: Streamline runs its own compute
// through a DX11-on-12 device, and without it DLSS reports unsupported for a reason that looks
// nothing like the cause. See runtime/backends/README.md.

#include <rescaleframe/dlss.h>

#include <windows.h>

#include <d3d11.h>

#include <cstdio>
#include <cstring>
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

std::vector<std::string> log_lines;

void collect(void* user, const char* message)
{
    (void)user;
    log_lines.emplace_back(message);
    std::fprintf(stderr, "[dlss] %s\n", message);
    std::fflush(stderr);
}

rsf_dlss_setup make_setup(const char* interposer)
{
    rsf_dlss_setup setup{};
    setup.struct_size = sizeof(setup);
    setup.abi_version = RSF_DLSS_ABI_VERSION;
    setup.interposer_path_utf8 = interposer;
    // Ace Combat 7 is a UE4.18 title and this is the identity NGX is given for it. Without an
    // identity the DLSS plugin loads and then refuses, which is a failure that looks like
    // unsupported hardware and is not.
    setup.engine = RSF_DLSS_ENGINE_UNREAL;
    setup.engine_version_utf8 = "4.18";
    setup.project_id_utf8 = "a3ed1f08-3542-4698-b85c-e1a9908e861a";
    setup.log = collect;
    return setup;
}

} // namespace

int main()
{
    // Whether the SDK was compiled in is reported, not guessed. A build without it must say so
    // instead of returning a failure that reads as "DLSS did not work here".
    const bool compiled_in = rsf_dlss_available() != 0u;
    std::fprintf(stderr, "streamline headers compiled in: %s\n", compiled_in ? "yes" : "no");

    rsf_dlss_setup setup = make_setup("Z:\\definitely\\not\\here\\sl.interposer.dll");

    setup.abi_version = RSF_DLSS_ABI_VERSION + 1u;
    check(rsf_dlss_load(&setup) == RSF_DLSS_ERROR_ABI_MISMATCH,
          "An incompatible ABI must be rejected.");
    setup.abi_version = RSF_DLSS_ABI_VERSION;

    setup.struct_size = 0;
    check(rsf_dlss_load(&setup) == RSF_DLSS_ERROR_INVALID_ARGUMENT,
          "A short setup structure must be rejected.");
    setup.struct_size = sizeof(setup);

    check(rsf_dlss_load(nullptr) == RSF_DLSS_ERROR_INVALID_ARGUMENT,
          "A missing setup structure must be rejected.");
    check(rsf_dlss_set_device(nullptr) == RSF_DLSS_ERROR_INVALID_ARGUMENT,
          "A missing device must be rejected.");

    // Nothing is loaded yet, so everything downstream must refuse rather than reach for a null
    // function pointer. This is the ordering the orchestrator has to follow.
    rsf_dlss_support support{};
    support.struct_size = sizeof(support);
    const rsf_dlss_result before_load = rsf_dlss_query_support(&support);
    check(before_load == (compiled_in ? RSF_DLSS_ERROR_NOT_READY : RSF_DLSS_ERROR_NOT_COMPILED),
          "Querying support before loading must be refused.");

    rsf_dlss_frame frame{};
    frame.struct_size = sizeof(frame);
    frame.abi_version = RSF_DLSS_ABI_VERSION;
    check(rsf_dlss_evaluate(nullptr, &frame) == RSF_DLSS_ERROR_INVALID_ARGUMENT,
          "Evaluating without a context must be rejected.");
    frame.abi_version = RSF_DLSS_ABI_VERSION + 1u;
    check(rsf_dlss_evaluate(reinterpret_cast<void*>(1), &frame) == RSF_DLSS_ERROR_ABI_MISMATCH,
          "An incompatible frame ABI must be rejected.");
    frame.abi_version = RSF_DLSS_ABI_VERSION;

    if (compiled_in) {
        // A resource is missing, which must be caught before anything is handed to Streamline.
        check(rsf_dlss_evaluate(reinterpret_cast<void*>(1), &frame) ==
                  RSF_DLSS_ERROR_INVALID_ARGUMENT,
              "Evaluating without the required resources must be rejected.");

        // The path does not exist. This must report a load failure, not take the process with it.
        check(rsf_dlss_load(&setup) == RSF_DLSS_ERROR_LOAD_FAILED,
              "An interposer that is not there must be reported as a load failure.");

        // Asked to verify a signature in a build that cannot verify one, the answer is no.
        // Refusing is the only honest outcome: reporting success would claim a check that never
        // ran, and loading anyway would defeat the request.
        rsf_dlss_setup signed_only = make_setup(setup.interposer_path_utf8);
        signed_only.require_signature = 1;
        check(rsf_dlss_load(&signed_only) == RSF_DLSS_ERROR_LOAD_FAILED,
              "A required signature that cannot be verified must refuse the load.");
    } else {
        check(rsf_dlss_load(&setup) == RSF_DLSS_ERROR_NOT_COMPILED,
              "A build without the SDK must say so rather than report a runtime failure.");
    }

    // The real thing, when the SDK is deployed on this machine.
    char directory[1024] = {};
    if (compiled_in && GetEnvironmentVariableA("RSF_STREAMLINE_BIN", directory,
                                               sizeof(directory)) != 0) {
        std::string interposer = std::string(directory) + "\\sl.interposer.dll";
        rsf_dlss_setup real = make_setup(interposer.c_str());
        real.plugin_directory_utf8 = directory;

        const rsf_dlss_result loaded = rsf_dlss_load(&real);
        if (loaded != RSF_DLSS_OK) {
            std::fprintf(stderr, "streamline did not initialise here (result %d), skipping\n",
                         int(loaded));
            return passed ? 0 : 1;
        }

        ID3D11Device* device = nullptr;
        ID3D11DeviceContext* context = nullptr;
        const D3D_FEATURE_LEVEL wanted[] = {D3D_FEATURE_LEVEL_11_0};
        D3D_FEATURE_LEVEL obtained{};
        if (FAILED(D3D11CreateDevice(nullptr, D3D_DRIVER_TYPE_HARDWARE, nullptr, 0, wanted, 1,
                                     D3D11_SDK_VERSION, &device, &obtained, &context)) ||
            !device) {
            std::fprintf(stderr, "no hardware device available, skipping the support query\n");
            rsf_dlss_shutdown();
            return passed ? 0 : 1;
        }

        check(rsf_dlss_set_device(device) == RSF_DLSS_OK,
              "Handing over a real device must succeed once Streamline is initialised.");

        support = rsf_dlss_support{};
        support.struct_size = sizeof(support);
        const rsf_dlss_result queried = rsf_dlss_query_support(&support);
        std::fprintf(stderr, "DLSS supported: %u (result %d), driver %u.%u requires %u.%u\n",
                     support.supported, int(queried), support.detected_driver_major,
                     support.detected_driver_minor, support.required_driver_major,
                     support.required_driver_minor);

        if (queried == RSF_DLSS_OK) {
            // Only meaningful on hardware that supports it, and then it is the first number the
            // integration actually depends on: the render size DLSS expects for an output size.
            rsf_dlss_plan plan{};
            plan.struct_size = sizeof(plan);
            plan.output_width = 2048;
            plan.output_height = 1152;
            plan.quality = RSF_DLSS_QUALITY_PERFORMANCE;
            check(rsf_dlss_plan_render_size(&plan) == RSF_DLSS_OK,
                  "DLSS must report a render size for a supported adapter.");
            check(plan.render_width > 0 && plan.render_width < plan.output_width,
                  "The render size must be smaller than the output size at performance quality.");
            std::fprintf(stderr, "performance quality renders %ux%u for 2048x1152\n",
                         plan.render_width, plan.render_height);
        }

        check(rsf_dlss_shutdown() == RSF_DLSS_OK, "Shutting down must succeed.");
        if (context) {
            context->Release();
        }
        device->Release();
    }

    return passed ? 0 : 1;
}

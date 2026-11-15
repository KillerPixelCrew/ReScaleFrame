// Validate the shipped DLL, rather than symbols linked into a test executable.
#include <windows.h>
#include <cstdio>

int main(int argc, char** argv) {
    if (argc != 2) return 2;
    HMODULE runtime = LoadLibraryA(argv[1]);
    if (!runtime) {
        std::fprintf(stderr, "Runtime load failed: %lu\n", GetLastError());
        return 1;
    }
    const char* names[] = {
        "rsf_get_runtime_version", "rsf_slvk_initialize",
        "rsf_latencyvk_instance_proc", "rsf_latencyvk_options",
        "rsf_latencyvk_cpu", "rsf_latencyvk_render", "rsf_latencyvk_present",
        "rsf_latencyvk_execution",
        "rsf_latencyvk_shutdown", "rsf_srvk_create", "rsf_srvk_plan",
        "rsf_srvk_evaluate", "rsf_srvk_retire_commands", "rsf_srvk_destroy",
        "rsf_srvk_fsr4_create", "rsf_srvk_fsr4_plan", "rsf_srvk_fsr4_evaluate",
        "rsf_srvk_fsr4_submitted", "rsf_srvk_fsr4_retire_commands", "rsf_srvk_fsr4_destroy",
        "rsf_fgvk_create_with_host", "rsf_fgvk_images", "rsf_fgvk_acquire",
        "rsf_fgvk_enable_options", "rsf_fgvk_prepare", "rsf_fgvk_hudless",
        "rsf_fgvk_submitted", "rsf_fgvk_present", "rsf_fgvk_destroy",
        "rsf_fgvk_present_source"
    };
    int result = 0;
    for (const char* name : names) {
        if (!GetProcAddress(runtime, name)) {
            std::fprintf(stderr, "Missing native Vulkan export: %s\n", name);
            result = 1;
        }
    }
    FreeLibrary(runtime);
    return result;
}

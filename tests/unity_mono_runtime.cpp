// SPDX-License-Identifier: GPL-3.0-only
/**
 * @file
 * Host an isolated Mono runtime to invoke the managed contract fixture's Run method.
 * Arguments provide the Mono DLL, framework search directory and test assembly.
 * Entry points are resolved dynamically; boxed integer results become the exit code.
 * The process owns the loaded runtime after domain cleanup. This runner is manual.
 */
#include <windows.h>
#include <cstdio>
#include <string>

template<typename T> T entry(HMODULE module, const char* name)
{
    return reinterpret_cast<T>(reinterpret_cast<void*>(GetProcAddress(module, name)));
}
int main(int argc, char** argv)
{
    if (argc != 4) { std::fprintf(stderr, "Usage: mono-contract mono-dll managed-framework-directory test-assembly\n"); return 2; }
    int size = MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, argv[1], -1, nullptr, 0);
    if (!size) return 1;
    std::wstring path(static_cast<size_t>(size), L'\0');
    MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, argv[1], -1, path.data(), size);
    HMODULE module = LoadLibraryExW(path.c_str(), nullptr, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS);
    if (!module) return 1;
    auto search = entry<void (*)(const char*)>(module, "mono_set_assemblies_path");
    auto init = entry<void* (*)(const char*, const char*)>(module, "mono_jit_init_version");
    auto open = entry<void* (*)(void*, const char*)>(module, "mono_domain_assembly_open");
    auto image = entry<void* (*)(void*)>(module, "mono_assembly_get_image");
    auto klass = entry<void* (*)(void*, const char*, const char*)>(module, "mono_class_from_name");
    auto method = entry<void* (*)(void*, const char*, int)>(module, "mono_class_get_method_from_name");
    auto invoke = entry<void* (*)(void*, void*, void**, void**)>(module, "mono_runtime_invoke");
    auto unbox = entry<void* (*)(void*)>(module, "mono_object_unbox");
    auto cleanup = entry<void (*)(void*)>(module, "mono_jit_cleanup");
    if (!search || !init || !open || !image || !klass || !method || !invoke || !unbox || !cleanup) return 1;
    search(argv[2]);
    void* domain = init("ReScaleFrame isolated Mono contract fixture", "v4.0.30319");
    if (!domain) return 1;
    void* assembly = open(domain, argv[3]);
    void* type = assembly ? klass(image(assembly), "", "Program") : nullptr;
    void* run = type ? method(type, "Run", 0) : nullptr;
    void* error = nullptr;
    void* result = run ? invoke(run, nullptr, nullptr, &error) : nullptr;
    int code = result && !error ? *static_cast<int*>(unbox(result)) : 1;
    if (error) std::fprintf(stderr, "Managed Mono fixture threw an exception.\n");
    cleanup(domain);
    // Process exit owns teardown of the hosted Mono runtime and its runtime-generated code.
    return code;
}

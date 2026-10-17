using System.Reflection;
using System.Runtime.InteropServices;

namespace ReScaleFrame.Vulkan;

/// <summary>One native resolver for the renderer assembly and its optional providers.</summary>
internal static class VulkanNativeLibrary
{
    private static string? ngxPath;

    static VulkanNativeLibrary() => NativeLibrary.SetDllImportResolver(
        typeof(VulkanNativeLibrary).Assembly, Resolve);

    /// <summary>Triggers the assembly's one-time native resolver initialization.</summary>
    internal static void EnsureRegistered() { }
    internal static void RegisterHostAssembly(Assembly assembly) => NativeLibrary.SetDllImportResolver(assembly, Resolve);
    /// <summary>Last exact NGX runtime path selected by this resolver, read with volatile semantics.</summary>
    internal static string? NgxPath => Volatile.Read(ref ngxPath);

    private static nint Resolve(string libraryName, Assembly assembly, DllImportSearchPath? searchPath)
    {
        if (OperatingSystem.IsWindows() && string.Equals(libraryName, "libxell.dll", StringComparison.OrdinalIgnoreCase))
        {
            string xell = Path.Combine(NativeRuntimePaths.DirectoryContaining("libxell.dll"), "libxell.dll");
            if (File.Exists(xell)) return NativeRuntimePaths.LoadLibrary(xell);
            if (NativeRuntimePaths.IsDeployedPayload)
                throw new DllNotFoundException($"VulkanStory XeLL runtime is missing: {xell}");
            return 0;
        }
        if (!string.Equals(libraryName, NgxShim.LibraryName, StringComparison.Ordinal)) return 0;
        string? binary = OperatingSystem.IsWindows() ? "VulkanStoryNgx.dll" :
            OperatingSystem.IsLinux() ? "libVulkanStoryNgx.so" : null;
        if (binary is null) return 0;

        string? configured = Environment.GetEnvironmentVariable(NgxShim.PathVariable);
        if (!string.IsNullOrWhiteSpace(configured))
        {
            if (!Path.IsPathFullyQualified(configured))
                throw new DllNotFoundException($"{NgxShim.PathVariable} must be an absolute path.");
            return LoadExact(Path.GetFullPath(configured));
        }

        string candidate = Path.Combine(NativeRuntimePaths.DirectoryContaining(binary), binary);
        if (File.Exists(candidate)) return LoadExact(candidate);

        if (NativeRuntimePaths.IsDeployedPayload)
            throw new DllNotFoundException($"VulkanStory NGX bridge is missing: {candidate}");
        return 0;
    }

    private static nint LoadExact(string path)
    {
        if (!File.Exists(path) || !NativeRuntimePaths.TryLoadLibrary(path, out nint handle))
            throw new DllNotFoundException($"VulkanStory NGX bridge could not be loaded: {path}");
        Volatile.Write(ref ngxPath, path);
        return handle;
    }
}

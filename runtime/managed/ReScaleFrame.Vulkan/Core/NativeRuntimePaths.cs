using System;
using System.IO;
using System.Runtime.InteropServices;

namespace ReScaleFrame.Vulkan;

/// <summary>
/// Locate native provider bridges and SDK runtimes from VulkanStory's private
/// package directory. Standalone development may keep them beside the assembly.
/// </summary>
internal static class NativeRuntimePaths
{
    /// <summary>Directory containing the renderer assembly, falling back to the application base directory.</summary>
    internal static string AssemblyDirectory =>
        Path.GetDirectoryName(typeof(NativeRuntimePaths).Assembly.Location) ?? AppContext.BaseDirectory;

    /// <summary>Whether the providers live in a private managed/native package layout.</summary>
    internal static bool IsDeployedPayload =>
        string.Equals(Path.GetFileName(AssemblyDirectory), "managed", StringComparison.OrdinalIgnoreCase);

    /// <summary>Sibling native directory for the supported process OS and architecture, or null for an unsupported RID.</summary>
    internal static string? PackageNativeDirectory
    {
        get
        {
            string? rid = RuntimeInformation.ProcessArchitecture switch
            {
                Architecture.X64 => OperatingSystem.IsWindows() ? "win-x64" :
                    OperatingSystem.IsLinux() ? "linux-x64" : null,
                Architecture.Arm64 => OperatingSystem.IsWindows() ? "win-arm64" :
                    OperatingSystem.IsLinux() ? "linux-arm64" : null,
                _ => null
            };
            return rid is null ? null : Path.Combine(
                Path.GetFullPath(Path.Combine(AssemblyDirectory, "..")), "native", rid);
        }
    }

    /// <summary>Selects a native runtime directory containing the requested filenames.</summary>
    /// <remarks>A deployed payload stays within its private RID directory even when dependencies are missing; development may fall back beside the assembly or application.</remarks>
    /// <param name="files">Bare native-runtime filenames, without directory components.</param>
    /// <returns>Selected directory; existence of every file is not guaranteed for an incomplete deployed payload.</returns>
    /// <exception cref="ArgumentException">A requested runtime name contains a directory component or is empty.</exception>
    internal static string DirectoryContaining(params string[] files)
    {
        foreach (string file in files)
            if (string.IsNullOrEmpty(file) || Path.GetFileName(file) != file)
                throw new ArgumentException("Native runtime names must be filenames.", nameof(files));

        string assembly = AssemblyDirectory;
        string? packaged = PackageNativeDirectory;
        if (packaged is not null)
        {
            if (ContainsAll(packaged, files)) return packaged;
            // A released payload must not accidentally load a similarly named
            // bridge from the game folder or another mod.
            if (IsDeployedPayload)
                return packaged;
        }
        if (ContainsAll(assembly, files)) return assembly;
        string application = AppContext.BaseDirectory;
        if (ContainsAll(application, files)) return application;
        return assembly;
    }

    private static bool ContainsAll(string directory, string[] files)
    {
        foreach (string file in files)
            if (!File.Exists(Path.Combine(directory, file))) return false;
        return true;
    }

    // SDK redistributables often have sibling DLL dependencies. Keep resolution
    // local to the selected module without changing the process DLL search path.
    /// <summary>Loads an absolute native module path with its sibling dependencies resolved locally.</summary>
    /// <param name="path">Fully qualified native module path.</param>
    /// <returns>Owned native module handle; the caller is responsible for its lifetime.</returns>
    /// <exception cref="ArgumentException">The supplied path is not fully qualified.</exception>
    internal static nint LoadLibrary(string path)
    {
        if (!Path.IsPathFullyQualified(path))
            throw new ArgumentException("Native runtime paths must be absolute.", nameof(path));
        return NativeLibrary.Load(path, typeof(NativeRuntimePaths).Assembly,
            OperatingSystem.IsWindows()
                ? DllImportSearchPath.UseDllDirectoryForDependencies | DllImportSearchPath.SafeDirectories
                : null);
    }

    /// <summary>Attempts native loading and converts missing/incompatible library failures to false.</summary>
    /// <param name="path">Absolute module path.</param>
    /// <param name="handle">Owned module handle on success; zero on a handled failure.</param>
    /// <returns>Whether the module loaded. Other errors, including an invalid path argument, propagate.</returns>
    internal static bool TryLoadLibrary(string path, out nint handle)
    {
        handle = 0;
        try { handle = LoadLibrary(path); return true; }
        catch (DllNotFoundException) { return false; }
        catch (BadImageFormatException) { return false; }
    }
}

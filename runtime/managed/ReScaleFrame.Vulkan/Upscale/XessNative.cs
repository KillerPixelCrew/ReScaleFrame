using System;
using System.IO;
using System.Runtime.InteropServices;
using Silk.NET.Vulkan;

namespace ReScaleFrame.Vulkan;

// Intel XeSS SDK 3.0.2, inc/xess/{xess.h,xess_vk.h}, pack(8), Windows x64 ABI.
// Keep the module loaded for the process: deferred contexts can outlive a selected provider.
/// <summary>Process-retained XeSS Vulkan exports with the pinned Windows x64 SDK ABI.</summary>
internal sealed unsafe class XessNative
{
    private static readonly object loadGate = new();
    private static XessNative? loaded;
    private readonly nint module;
    /// <summary>Native required-instance-extension query; returned extension pointers borrow SDK storage.</summary>
    public readonly delegate* unmanaged[Cdecl]<uint*, byte***, uint*, int> InstanceExtensions;
    /// <summary>Native required-device-extension query for the selected instance/physical device.</summary>
    public readonly delegate* unmanaged[Cdecl]<nint, nint, uint*, byte***, int> DeviceExtensions;
    /// <summary>Native feature-chain negotiation callback that can modify the caller's chain.</summary>
    public readonly delegate* unmanaged[Cdecl]<nint, nint, void**, int> DeviceFeatures;
    /// <summary>Native reconstruction-context creation entry point; returned contexts are owned by the backend.</summary>
    public readonly delegate* unmanaged[Cdecl]<nint, nint, nint, nint*, int> Create;
    /// <summary>Native reconstruction initialization entry point for the selected output extent and quality.</summary>
    public readonly delegate* unmanaged[Cdecl]<nint, XessInit*, int> Init;
    /// <summary>Native XeSS execution entry point using the active Vulkan command buffer.</summary>
    public readonly delegate* unmanaged[Cdecl]<nint, nint, XessExecute*, int> Execute;
    /// <summary>Native reconstruction-context destruction entry point; call only after referencing GPU work completes.</summary>
    public readonly delegate* unmanaged[Cdecl]<nint, int> Destroy;
    /// <summary>Native XeSS optimal/minimum/maximum input-resolution query.</summary>
    public readonly delegate* unmanaged[Cdecl]<nint, XessSize*, int, XessSize*, XessSize*, XessSize*, int> Resolution;
    /// <summary>Read-only pinned SDK jitter-scale query; coordinates are returned through float pointers.</summary>
    public readonly delegate* unmanaged[Cdecl]<nint, float*, float*, int> JitterScale;
    /// <summary>Read-only pinned SDK velocity-scale query; coordinates are returned through float pointers.</summary>
    public readonly delegate* unmanaged[Cdecl]<nint, float*, float*, int> VelocityScale;

    private XessNative(nint module)
    {
        this.module = module;
        InstanceExtensions = (delegate* unmanaged[Cdecl]<uint*, byte***, uint*, int>)Export("xessVKGetRequiredInstanceExtensions");
        DeviceExtensions = (delegate* unmanaged[Cdecl]<nint, nint, uint*, byte***, int>)Export("xessVKGetRequiredDeviceExtensions");
        DeviceFeatures = (delegate* unmanaged[Cdecl]<nint, nint, void**, int>)Export("xessVKGetRequiredDeviceFeatures");
        Create = (delegate* unmanaged[Cdecl]<nint, nint, nint, nint*, int>)Export("xessVKCreateContext");
        Init = (delegate* unmanaged[Cdecl]<nint, XessInit*, int>)Export("xessVKInit");
        Execute = (delegate* unmanaged[Cdecl]<nint, nint, XessExecute*, int>)Export("xessVKExecute");
        Destroy = (delegate* unmanaged[Cdecl]<nint, int>)Export("xessDestroyContext");
        Resolution = (delegate* unmanaged[Cdecl]<nint, XessSize*, int, XessSize*, XessSize*, XessSize*, int>)Export("xessGetOptimalInputResolution");
        JitterScale = (delegate* unmanaged[Cdecl]<nint, float*, float*, int>)Export("xessGetJitterScale");
        VelocityScale = (delegate* unmanaged[Cdecl]<nint, float*, float*, int>)Export("xessGetVelocityScale");
    }
    private nint Export(string name) => NativeLibrary.GetExport(module, name);
    /// <summary>Returns process-cached XeSS exports or loads the pinned Windows x64 runtime from an absolute path.</summary>
    /// <remarks>A successful module stays loaded so deferred contexts can safely invoke its exports.</remarks>
    /// <returns>Loaded exports, or null with availability/load detail.</returns>
    public static XessNative? TryLoad(out string? error)
    {
        lock (loadGate)
        {
            error = null;
            if (loaded != null) return loaded;
            if (!OperatingSystem.IsWindows() || IntPtr.Size != 8)
            { error = "XeSS-SR requires the Windows x64 runtime"; return null; }
            string path = Environment.GetEnvironmentVariable("VULKANSTORY_XESS_LIBRARY") ??
                Path.Combine(NativeRuntimePaths.DirectoryContaining("libxess.dll"), "libxess.dll");
            if (!Path.IsPathFullyQualified(path))
            { error = "VULKANSTORY_XESS_LIBRARY must be an absolute path"; return null; }
            nint handle = 0;
            try { handle = NativeRuntimePaths.LoadLibrary(Path.GetFullPath(path)); return loaded = new XessNative(handle); }
            catch (Exception e) when (e is DllNotFoundException or BadImageFormatException or EntryPointNotFoundException)
            { if (handle != 0) NativeLibrary.Free(handle); error = "libxess.dll unavailable: " + e.Message; return null; }
        }
    }
}

/// <summary>Sequential width and height pair in the pinned XeSS C ABI.</summary>
[StructLayout(LayoutKind.Sequential, Pack = 8)]
internal struct XessSize { public uint Width, Height; public XessSize(int w, int h) { Width = (uint)w; Height = (uint)h; } }
/// <summary>XeSS context initialization parameters, output extent and optional heap/cache handles.</summary>
[StructLayout(LayoutKind.Sequential, Pack = 8)]
internal struct XessInit
{
    public XessSize Output;
    public int Quality;
    public uint Flags, CreationNodeMask, VisibleNodeMask;
    public ulong BufferHeap, BufferOffset, TextureHeap, TextureOffset, PipelineCache;
}
/// <summary>Borrowed Vulkan image-view and subresource metadata for a XeSS input or output.</summary>
[StructLayout(LayoutKind.Sequential, Pack = 8)]
internal struct XessImage
{
    public ulong View, Image;
    public ImageSubresourceRange Range;
    public Format Format;
    public uint Width, Height;

}
/// <summary>Sequential XeSS execution resources, jitter, exposure, reset and subrectangle parameters.</summary>
[StructLayout(LayoutKind.Sequential, Pack = 8)]
internal struct XessExecute
{
    public XessImage Color, Velocity, Depth, Exposure, ResponsiveMask, Output;
    public float JitterX, JitterY, ExposureScale;
    public uint Reset, Width, Height;
    public XessSize ColorBase, MotionBase, DepthBase, MaskBase, Reserved, OutputBase;
}

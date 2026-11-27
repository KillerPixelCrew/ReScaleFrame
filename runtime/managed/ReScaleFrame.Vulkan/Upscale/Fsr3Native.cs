using System;
using System.IO;
using System.Runtime.InteropServices;
using Silk.NET.Vulkan;

using Semaphore = Silk.NET.Vulkan.Semaphore;

namespace ReScaleFrame.Vulkan;

// AMD FidelityFX SDK 1.1.4 signed Vulkan DLL, wrapped by VulkanStoryFsr3.dll.
// Both modules stay loaded because context destruction is deferred on the GPU timeline.
/// <summary>Process-retained FidelityFX Vulkan bridge exports shared by reconstruction, interpolation and swapchain contexts.</summary>
internal sealed unsafe class Fsr3Native
{
    private static readonly object loadGate = new();
    private static Fsr3Native? loaded;
    private readonly nint sdk;
    private readonly nint bridge;
    /// <summary>Opens the signed FidelityFX Vulkan SDK module for use by the bridge.</summary>
    public readonly delegate* unmanaged[Cdecl]<nint, int> Open;
    /// <summary>Native FidelityFX input-resolution query for output dimensions and quality.</summary>
    public readonly delegate* unmanaged[Cdecl]<uint, uint, uint, uint*, uint*, int> Plan;
    /// <summary>Native reconstruction-context creation entry point; returned contexts are owned by the backend.</summary>
    public readonly delegate* unmanaged[Cdecl]<nint, nint, uint, uint, uint, uint, nint*, int> Create;
    /// <summary>Native reconstruction evaluation entry point using caller-owned frame resources.</summary>
    public readonly delegate* unmanaged[Cdecl]<nint, Fsr3Frame*, int> Evaluate;
    /// <summary>Native reconstruction-context destruction entry point; call only after referencing GPU work completes.</summary>
    public readonly delegate* unmanaged[Cdecl]<nint, int> Destroy;
    /// <summary>Native FidelityFX interpolation-context creation entry point.</summary>
    public readonly delegate* unmanaged[Cdecl]<nint, nint, uint, uint, uint, nint*, int> CreateFrameGeneration;
    /// <summary>Native FidelityFX interpolation evaluation entry point.</summary>
    public readonly delegate* unmanaged[Cdecl]<nint, Fsr3FgFrame*, int> EvaluateFrameGeneration;
    /// <summary>Records direct FidelityFX interpolation without the SDK swapchain presentation proxy.</summary>
    public readonly delegate* unmanaged[Cdecl]<nint, Fsr3FgFrame*, int> EvaluateDirectFrameGeneration;
    /// <summary>Native FidelityFX interpolation disable entry point for a live swapchain context.</summary>
    public readonly delegate* unmanaged[Cdecl]<nint, nint, int> DisableFrameGeneration;
    /// <summary>Native FidelityFX interpolation-context destruction after GPU-safe retirement.</summary>
    public readonly delegate* unmanaged[Cdecl]<nint, int> DestroyFrameGeneration;
    /// <summary>Native FidelityFX swapchain-context creation entry point with explicit queue topology.</summary>
    public readonly delegate* unmanaged[Cdecl]<PhysicalDevice, Device, Queue, Queue, Queue, Queue, uint,
        SwapchainCreateInfoKHR*, nint*, int> CreateSwapchain;
    /// <summary>Native query for the swapchain handle retained by the FidelityFX context.</summary>
    public readonly delegate* unmanaged[Cdecl]<nint, SwapchainKHR> SwapchainHandle;
    /// <summary>Native FidelityFX swapchain recreation through the same presentation owner.</summary>
    public readonly delegate* unmanaged[Cdecl]<nint, SwapchainCreateInfoKHR*, SwapchainKHR*, Result> RecreateSwapchain;
    /// <summary>Provider-owned swapchain image enumeration entry point.</summary>
    public readonly delegate* unmanaged[Cdecl]<nint, SwapchainKHR, uint*, Image*, Result> GetSwapchainImages;
    /// <summary>Provider-owned swapchain image acquisition entry point.</summary>
    public readonly delegate* unmanaged[Cdecl]<nint, SwapchainKHR, ulong, Semaphore, Fence, uint*, Result> AcquireSwapchain;
    /// <summary>Provider-owned Vulkan presentation entry point.</summary>
    public readonly delegate* unmanaged[Cdecl]<nint, Queue, PresentInfoKHR*, Result> PresentSwapchain;
    /// <summary>Checked native disable/drain preparation before swapchain release.</summary>
    public readonly delegate* unmanaged[Cdecl]<nint, SwapchainKHR, int> PrepareSwapchainDestroy;
    /// <summary>Checked native swapchain-handle destruction entry point.</summary>
    public readonly delegate* unmanaged[Cdecl]<nint, SwapchainKHR, int> DestroySwapchainChain;
    /// <summary>Native query for the provider's latest reported presentation count.</summary>
    public readonly delegate* unmanaged[Cdecl]<nint, SwapchainKHR, ulong> SwapchainLastPresentCount;
    /// <summary>Checked native presentation-context destruction entry point.</summary>
    public readonly delegate* unmanaged[Cdecl]<nint, int> DestroySwapchain;

    private Fsr3Native(nint sdk, nint bridge)
    {
        this.sdk = sdk;
        this.bridge = bridge;
        Open = (delegate* unmanaged[Cdecl]<nint, int>)Export("rsf_vk_fsr3_open");
        Plan = (delegate* unmanaged[Cdecl]<uint, uint, uint, uint*, uint*, int>)Export("rsf_vk_fsr3_plan");
        Create = (delegate* unmanaged[Cdecl]<nint, nint, uint, uint, uint, uint, nint*, int>)Export("rsf_vk_fsr3_create");
        Evaluate = (delegate* unmanaged[Cdecl]<nint, Fsr3Frame*, int>)Export("rsf_vk_fsr3_evaluate_with_composition");
        Destroy = (delegate* unmanaged[Cdecl]<nint, int>)Export("rsf_vk_fsr3_destroy");
        CreateFrameGeneration = (delegate* unmanaged[Cdecl]<nint, nint, uint, uint, uint, nint*, int>)Export("rsf_vk_fsr3_fg_create");
        EvaluateFrameGeneration = (delegate* unmanaged[Cdecl]<nint, Fsr3FgFrame*, int>)Export("rsf_vk_fsr3_fg_evaluate");
        EvaluateDirectFrameGeneration = (delegate* unmanaged[Cdecl]<nint, Fsr3FgFrame*, int>)Export("rsf_vk_fsr3_fg_evaluate_direct");
        DisableFrameGeneration = (delegate* unmanaged[Cdecl]<nint, nint, int>)Export("rsf_vk_fsr3_fg_disable");
        DestroyFrameGeneration = (delegate* unmanaged[Cdecl]<nint, int>)Export("rsf_vk_fsr3_fg_destroy");
        CreateSwapchain = (delegate* unmanaged[Cdecl]<PhysicalDevice, Device, Queue, Queue, Queue, Queue, uint,
            SwapchainCreateInfoKHR*, nint*, int>)Export("rsf_vk_fsr3_swapchain_create");
        SwapchainHandle = (delegate* unmanaged[Cdecl]<nint, SwapchainKHR>)Export("rsf_vk_fsr3_swapchain_handle");
        RecreateSwapchain = (delegate* unmanaged[Cdecl]<nint, SwapchainCreateInfoKHR*, SwapchainKHR*, Result>)Export("rsf_vk_fsr3_swapchain_recreate");
        GetSwapchainImages = (delegate* unmanaged[Cdecl]<nint, SwapchainKHR, uint*, Image*, Result>)Export("rsf_vk_fsr3_swapchain_get_images");
        AcquireSwapchain = (delegate* unmanaged[Cdecl]<nint, SwapchainKHR, ulong, Semaphore, Fence, uint*, Result>)Export("rsf_vk_fsr3_swapchain_acquire");
        PresentSwapchain = (delegate* unmanaged[Cdecl]<nint, Queue, PresentInfoKHR*, Result>)Export("rsf_vk_fsr3_swapchain_present");
        // Required symbols prevent pairing checked teardown with an old bridge.
        PrepareSwapchainDestroy = (delegate* unmanaged[Cdecl]<nint, SwapchainKHR, int>)Export("rsf_vk_fsr3_swapchain_prepare_destroy");
        DestroySwapchainChain = (delegate* unmanaged[Cdecl]<nint, SwapchainKHR, int>)Export("rsf_vk_fsr3_swapchain_destroy_chain_checked");
        SwapchainLastPresentCount = (delegate* unmanaged[Cdecl]<nint, SwapchainKHR, ulong>)Export("rsf_vk_fsr3_swapchain_last_present_count");
        DestroySwapchain = (delegate* unmanaged[Cdecl]<nint, int>)Export("rsf_vk_fsr3_swapchain_destroy");
    }
    private nint Export(string symbol) => NativeLibrary.GetExport(bridge, symbol);

    /// <summary>Returns the process-cached FidelityFX Vulkan bridge or loads it from the private native directory.</summary>
    /// <remarks>Successful native loading remains cached so deferred provider contexts never outlive their export addresses.</remarks>
    /// <returns>Loaded exports, or null with a load-failure detail.</returns>
    public static Fsr3Native? TryLoad(out string? error)
    {
        lock (loadGate)
        {
            error = null;
            if (loaded != null) return loaded;
            if (!OperatingSystem.IsWindows() || IntPtr.Size != 8)
            { error = "the FSR 3.1 Vulkan runtime is available on Windows x64 in this build"; return null; }
            string directory = NativeRuntimePaths.DirectoryContaining(
                "amd_fidelityfx_vk.dll", "VulkanStoryFsr3.dll");
            string sdkPath = Environment.GetEnvironmentVariable("VULKANSTORY_FSR3_LIBRARY") ??
                Path.Combine(directory, "amd_fidelityfx_vk.dll");
            string bridgePath = Environment.GetEnvironmentVariable("VULKANSTORY_FSR3_BRIDGE") ??
                Path.Combine(directory, "VulkanStoryFsr3.dll");
            nint sdk = 0, bridge = 0;
            try
            {
                sdk = NativeRuntimePaths.LoadLibrary(Path.GetFullPath(sdkPath));
                bridge = NativeRuntimePaths.LoadLibrary(Path.GetFullPath(bridgePath));
                var candidate = new Fsr3Native(sdk, bridge);
                if (candidate.Open(sdk) != 0) throw new EntryPointNotFoundException("FSR 3.1 Vulkan exports are missing");
                return loaded = candidate;
            }
            catch (Exception exception) when (exception is DllNotFoundException or BadImageFormatException or EntryPointNotFoundException)
            {
                if (bridge != 0) NativeLibrary.Free(bridge);
                if (sdk != 0) NativeLibrary.Free(sdk);
                error = "FSR 3.1 Vulkan runtime unavailable: " + exception.Message;
                return null;
            }
        }
    }
}

/// <summary>Borrowed Vulkan image metadata matching the FidelityFX bridge C ABI.</summary>
[StructLayout(LayoutKind.Sequential, Pack = 8)]
internal partial struct Fsr3Image
{
    public ulong Image;
    public uint Width, Height, Format;
}

/// <summary>184-byte C-ABI reconstruction inputs; six 24-byte image records precede jitter at byte 152.</summary>
[StructLayout(LayoutKind.Sequential, Pack = 8)]
internal struct Fsr3Frame
{
    public nint Commands;
    public Fsr3Image Color, Depth, Motion, Output;
    /// <summary>Pre-composition material reactive mask, normalized and clamped to AMD's recommended maximum.</summary>
    public Fsr3Image Reactive;
    /// <summary>Independent current-frame transparency and animated composition coverage.</summary>
    public Fsr3Image Composition;
    public float JitterX, JitterY, DeltaMs, NearPlane, FarPlane, FovRadians;
    public uint Reset;
}

/// <summary>Sequential C-ABI interpolation resources, frame identity, camera vectors and swapchain context.</summary>
[StructLayout(LayoutKind.Sequential, Pack = 8)]
internal struct Fsr3FgFrame
{
    public nint Commands;
    public Fsr3Image Color, Depth, Motion, Output, Hudless;
    public float JitterX, JitterY, DeltaMs, NearPlane, FarPlane, FovRadians;
    public ulong FrameId;
    public uint Reset;
    public float CameraPosX, CameraPosY, CameraPosZ;
    public float CameraUpX, CameraUpY, CameraUpZ;
    public float CameraRightX, CameraRightY, CameraRightZ;
    public float CameraForwardX, CameraForwardY, CameraForwardZ;
    public Fsr3Image Ui;
    public nint SwapchainContext;
}

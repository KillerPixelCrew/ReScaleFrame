using System;
using VulkanStory.Contracts;

namespace ReScaleFrame.Vulkan;

/// <summary>One FidelityFX frame interpolation context, retired on the frame timeline.</summary>
internal sealed unsafe class Fsr3FrameGeneration : IDisposable
{
    private readonly Fsr3Native api;
    private nint handle;
    private bool needsReset = true;

    private Fsr3FrameGeneration(Fsr3Native api, nint handle, uint width, uint height)
    {
        this.api = api;
        this.handle = handle;
        Width = width;
        Height = height;
    }

    /// <summary>Interpolation output width in pixels.</summary>
    public uint Width { get; }
    /// <summary>Interpolation output height in pixels.</summary>
    public uint Height { get; }
    /// <summary>Whether the native interpolation context is still owned.</summary>
    public bool IsValid => handle != 0;
    /// <summary>Last monotonically incremented interpolation evaluation ID.</summary>
    public ulong FrameId { get; private set; }

    /// <summary>Creates a FidelityFX interpolation context using the renderer-selected format and device handles.</summary>
    /// <returns>Zero on success, a negative local availability code or the native bridge failure code.</returns>
    public static int Create(IFrameGenerationDevice device, uint width, uint height,
        out Fsr3FrameGeneration? feature)
    {
        feature = null;
        if (width == 0 || height == 0) return -1;
        Fsr3Native? api = Fsr3Native.TryLoad(out _);
        if (api == null) return -2;
        device.UpscalerHandles(out _, out IntPtr physical, out IntPtr logical);
        if (physical == IntPtr.Zero || logical == IntPtr.Zero) return -3;
        nint handle = 0;
        int result = api.CreateFrameGeneration(logical, physical, width, height,
            (uint)device.Fsr3ProxyFormat, &handle);
        if (result != 0 || handle == 0) return result != 0 ? result : -4;
        feature = new Fsr3FrameGeneration(api, handle, width, height);
        return 0;
    }

    /// <summary>Records interpolation for matching scene, UI, depth and motion resources.</summary>
    /// <remarks>Temporal reset is forced after creation or disablement and clears after a successful dispatch.</remarks>
    /// <returns>The native dispatch result code, or -8 when the context is absent.</returns>
    public int Evaluate(IFrameGenerationDevice device, int backbufferId, int depthId, int motionId,
        int motionRgId, int hudlessId, int uiId,
        int uprightSceneId, int uprightUiId, int uprightDepthId, int uprightMotionId,
        int uprightOutputId, int outputId,
        in TemporalProviderFrame temporal)
    {
        if (handle == 0) return -8;
        ulong frameId = ++FrameId;
        TemporalProviderFrame current = temporal with { Reset = temporal.Reset || needsReset };
        int result = device.EvaluateFsr3FrameGeneration(api, handle, backbufferId, depthId,
            motionId, motionRgId, hudlessId, uiId, uprightSceneId, uprightUiId,
            uprightDepthId, uprightMotionId, uprightOutputId, outputId, current, frameId);
        if (result == 0) needsReset = false;
        return result;
    }

    /// <summary>Disables interpolation for the supplied swapchain context and resets history on success.</summary>
    /// <returns>The native result code; an already absent feature returns zero.</returns>
    public int Disable(nint swapchainContext)
    {
        int result = handle != 0 ? api.DisableFrameGeneration(handle, swapchainContext) : 0;
        if (result == 0) needsReset = true;
        return result;
    }

    /// <inheritdoc/>
    public void Dispose()
    {
        nint current = handle;
        if (current == 0) return;
        int result = api.DestroyFrameGeneration(current);
        if (result != 0)
            throw new InvalidOperationException("FidelityFX frame generation destruction failed (" + result + ").");
        handle = 0;
    }
}

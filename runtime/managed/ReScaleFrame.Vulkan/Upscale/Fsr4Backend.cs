using System;
using System.Runtime.InteropServices;
using Silk.NET.Vulkan;
using VulkanStory.Contracts;

namespace ReScaleFrame.Vulkan;

/// <summary>Runs FidelityFX 4 through a DX12 bridge on the Vulkan-selected adapter.</summary>
/// <remarks>Owns the bridge module, DX12 runtime, shared image sets and renderer motion texture. Shared resources drain both GPU APIs before release.</remarks>
internal sealed class Fsr4Backend : IUpscalerBackend
{
    private readonly Action<string> log;
    private nint bridge;
    private nint physical;
    private IUpscalerDevice? device;
    private Fsr4Runtime? runtime;
    private IFsr4SharedFrames? shared;
    private UpscalerPlan initializedPlan;
    private int motion;
    private bool ready, firstFrame;

    /// <inheritdoc/>
    public string Id => "fsr4";
    /// <inheritdoc/>
    public bool Active => ready && Unavailable == null;
    /// <inheritdoc/>
    public string? Unavailable { get; private set; }
    /// <inheritdoc/>
    public IDeviceRequirementContributor? Requirements => null;

    /// <summary>Loads the signed-runtime bridge from the selected private native directory.</summary>
    /// <param name="log">Destination for provider creation and dispatch failures.</param>
    public Fsr4Backend(Action<string> log)
    {
        this.log = log;
        if (!Fsr4Runtime.TryLoad(out bridge, out string reason)) Unavailable = reason;
    }

    /// <inheritdoc/>
    public bool BringUp(IUpscalerDevice target, nint instance, nint physicalDevice, nint logicalDevice)
    {
        if (bridge == 0) return false;
        if (Fsr4Runtime.Probe(bridge, physicalDevice) != 0)
        {
            Unavailable = "FSR 4 is not supported on this GPU";
            return false;
        }
        device = target;
        physical = physicalDevice;
        ready = true;
        return true;
    }

    /// <inheritdoc/>
    public bool TryPlan(int displayWidth, int displayHeight, string quality,
        float lodBiasOffset, out UpscalerPlan plan)
    {
        plan = default;
        if (!Active || displayWidth <= 0 || displayHeight <= 0) return false;
        float ratio = quality?.ToLowerInvariant() switch
        {
            "dlaa" or "native" => 1f,
            "balanced" => 1.7f,
            "performance" => 2f,
            "ultraperformance" => 3f,
            _ => 1.5f,
        };
        quality = string.IsNullOrWhiteSpace(quality) ? "quality" : quality;
        int renderWidth = Math.Max(1, (int)(displayWidth / ratio));
        int renderHeight = Math.Max(1, (int)(displayHeight / ratio));
        plan = new UpscalerPlan(renderWidth, renderHeight, displayWidth, displayHeight,
            quality, null, lodBiasOffset);
        return plan.IsValid;
    }

    /// <inheritdoc/>
    public bool Evaluate(in UpscalerPlan plan, in UpscalerFrame frame, out string? error)
    {
        error = null;
        if (!Active || !plan.IsValid)
        {
            error = Unavailable ?? "FSR 4 is unavailable";
            return false;
        }
        if (initializedPlan != plan)
        {
            RetireFeature();
            if (!Fsr4Runtime.TryCreate(bridge, physical, plan, out runtime, out string reason))
                return Fail(reason, out error);
            if (!device!.TryCreateFsr4SharedFrames(runtime!, plan, out shared, out reason))
            {
                runtime!.Dispose(); runtime = null;
                return Fail(reason, out error);
            }
            motion = device.CreateUpscaleTexture(plan.RenderWidth, plan.RenderHeight,
                Format.R16G16Sfloat, false);
            initializedPlan = plan;
            firstFrame = true;
        }
        int result = device!.EvaluateFsr4(runtime!, shared!, motion, plan, frame, firstFrame);
        firstFrame = false;
        if (result == 0) return true;
        return Fail("FSR 4 DX12 dispatch failed (" + result.ToString("X8") + ")", out error);
    }

    /// <summary>Persists and logs a provider-unavailability reason while returning a failed operation.</summary>
    private bool Fail(string reason, out string? error)
    {
        Unavailable = reason;
        error = reason;
        log("[VulkanStory] " + reason);
        return false;
    }

    /// <inheritdoc/>
    public void RetireFeature()
    {
        // A shared DX12 image can still be used by either GPU queue. Wait for
        // both before releasing the imported Vulkan memory and the SDK context.
        // Shared disposal drains Vulkan as well before preparing SDK release.
        if (shared == null) runtime?.PrepareRelease();
        shared?.Dispose(); shared = null;
        runtime?.Dispose(); runtime = null;
        if (motion != 0) { device!.DeleteTexture(motion); motion = 0; }
        initializedPlan = default;
    }

    /// <inheritdoc/>
    public void Shutdown()
    {
        RetireFeature();
        ready = false;
        if (bridge != 0) { NativeLibrary.Free(bridge); bridge = 0; }
    }
    /// <inheritdoc/>
    public void Dispose() => Shutdown();
}

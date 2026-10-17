using System;
using Silk.NET.Vulkan;
using VulkanStory.Contracts;

namespace ReScaleFrame.Vulkan;

/// <summary>FidelityFX 3.1 reconstruction backend with Vulkan feature negotiation and plan-dependent context ownership.</summary>
/// <remarks>Called by the renderer owner. Context release is deferred on its frame timeline; motion textures use the renderer deletion path.</remarks>
internal sealed unsafe class Fsr3Backend : IUpscalerBackend, IDeviceRequirementContributor
{
    private readonly Fsr3Native? api;
    private readonly Action<string> log;
    private IUpscalerDevice? device;
    private nint physical, logical;
    private Fsr3Context? context;
    private UpscalerPlan initializedPlan;
    private int motion;
    private bool ready, firstFrame;
    private bool shaderInt16, shaderFloat16;
    /// <inheritdoc/>
    public string Id => "fsr3";
    /// <inheritdoc/>
    public string Name => "FSR 3.1";
    /// <inheritdoc/>
    public bool Active => ready && Unavailable == null;
    /// <inheritdoc/>
    public string? Unavailable { get; private set; }
    /// <inheritdoc/>
    public IDeviceRequirementContributor? Requirements => api == null ? null : this;

    /// <summary>Loads the optional FidelityFX bridge and records its availability.</summary>
    /// <param name="log">Destination for SDK-operation failures.</param>
    public Fsr3Backend(Action<string> log)
    {
        this.log = log;
        api = Fsr3Native.TryLoad(out string? error);
        Unavailable = error;
    }
    private bool Check(int result, string operation)
    {
        if (result == 0) return true;
        Unavailable = operation + " failed (AMD FidelityFX " + result + ")";
        log("[VulkanStory] " + Unavailable);
        return false;
    }
    /// <inheritdoc/>
    public void ContributeInstanceExtensions(InstanceRequirements requirements) { }
    /// <inheritdoc/>
    public void ContributeDeviceRequirements(DeviceRequirements requirements)
    {
        // The SDK chooses its FP16 shader permutation from physical-device
        // support, so the matching Vulkan features must be enabled at creation.
        shaderInt16 = requirements.Api.GetPhysicalDeviceFeatures(requirements.PhysicalDevice).ShaderInt16;
        var supported = new PhysicalDeviceVulkan12Features
        {
            SType = StructureType.PhysicalDeviceVulkan12Features,
        };
        requirements.QueryFeatures(&supported);
        shaderFloat16 = supported.ShaderFloat16;
    }
    /// <inheritdoc/>
    public void FinalizeDeviceFeatures(DeviceRequirements requirements, void** features)
    {
        for (BaseOutStructure* node = (BaseOutStructure*)*features; node != null; node = node->PNext)
        {
            if (node->SType == StructureType.PhysicalDeviceFeatures2)
                ((PhysicalDeviceFeatures2*)node)->Features.ShaderInt16 = shaderInt16;
            if (node->SType == StructureType.PhysicalDeviceVulkan12Features)
                ((PhysicalDeviceVulkan12Features*)node)->ShaderFloat16 = shaderFloat16;
        }
    }
    /// <inheritdoc/>
    public bool BringUp(IUpscalerDevice target, nint instance, nint physicalDevice, nint logicalDevice)
    {
        if (api == null) return false;
        device = target; physical = physicalDevice; logical = logicalDevice;
        ready = true;
        return true;
    }
    /// <summary>Maps renderer quality names to the FidelityFX bridge quality enum; unknown names select Quality.</summary>
    internal static uint QualityOf(string? quality) => quality?.ToLowerInvariant() switch
    {
        "dlaa" or "native" => 0, "balanced" => 2, "performance" => 3,
        "ultraperformance" => 4, _ => 1,
    };
    /// <inheritdoc/>
    public bool TryPlan(int displayWidth, int displayHeight, string quality,
        float lodBiasOffset, out UpscalerPlan plan)
    {
        plan = default;
        if (!Active || displayWidth <= 0 || displayHeight <= 0) return false;
        uint renderWidth = 0, renderHeight = 0;
        quality = string.IsNullOrWhiteSpace(quality) ? "quality" : quality;
        if (!Check(api!.Plan((uint)displayWidth, (uint)displayHeight, QualityOf(quality),
            &renderWidth, &renderHeight), "FSR 3.1 resolution query")) return false;
        plan = new UpscalerPlan((int)renderWidth, (int)renderHeight, displayWidth, displayHeight,
            quality, null, lodBiasOffset);
        return plan.IsValid;
    }
    /// <inheritdoc/>
    public bool Evaluate(in UpscalerPlan plan, in UpscalerFrame frame, out string? error)
    {
        error = null;
        if (!Active || !plan.IsValid) { error = Unavailable ?? "FSR 3.1 is unavailable"; return false; }
        if (initializedPlan != plan)
        {
            if (initializedPlan.IsValid) RetireFeature();
            nint handle = 0;
            if (!Check(api!.Create(logical, physical, (uint)plan.RenderWidth, (uint)plan.RenderHeight, (uint)plan.DisplayWidth,
                (uint)plan.DisplayHeight, &handle), "FSR 3.1 context creation"))
            { error = Unavailable; return false; }
            context = new Fsr3Context(api, handle);
            motion = device!.CreateUpscaleTexture(plan.RenderWidth, plan.RenderHeight,
                Format.R16G16Sfloat, false);
            initializedPlan = plan;
            firstFrame = true;
        }
        int result = device!.EvaluateFsr3(api!, context!.Handle, motion, frame, firstFrame);
        firstFrame = false;
        if (Check(result, "FSR 3.1 dispatch")) return true;
        error = Unavailable;
        return false;
    }
    /// <inheritdoc/>
    public void RetireFeature()
    {
        if (context != null) { device!.RetireUpscalerResource(context); context = null; }
        if (motion != 0) { device!.DeleteTexture(motion); motion = 0; }
        initializedPlan = default;
    }
    /// <inheritdoc/>
    public void Shutdown() { RetireFeature(); ready = false; }
    /// <inheritdoc/>
    public void Dispose() => Shutdown();

    /// <summary>Owns one native FidelityFX SR context until GPU-safe retirement.</summary>
    /// <remarks>A failed checked destruction retains the handle and makes later release attempts terminal.</remarks>
    private sealed class Fsr3Context(Fsr3Native api, nint handle) : IDisposable
    {
        private Exception? releaseFailure;
        public nint Handle { get; private set; } = handle;
        /// <inheritdoc/>
        public void Dispose()
        {
            if (releaseFailure != null)
                throw new InvalidOperationException("FidelityFX SR release failed; cleanup is terminal.", releaseFailure);
            if (Handle == 0) return;
            int result = api.Destroy(Handle);
            if (result != 0)
            {
                releaseFailure = new InvalidOperationException("FidelityFX SR context destruction failed (" + result + "); ownership retained.");
                throw releaseFailure;
            }
            Handle = 0;
        }
    }
}

using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using Silk.NET.Vulkan;
using VulkanStory.Contracts;

namespace ReScaleFrame.Vulkan;

/// <summary>XeSS Vulkan reconstruction backend that contributes SDK requirements before device creation.</summary>
/// <remarks>Plan changes retire the previous context through the renderer timeline. SDK warning results are treated as successful operations.</remarks>
internal sealed unsafe class XessBackend : IUpscalerBackend, IDeviceRequirementContributor
{
    // xess.h: ENABLE_AUTOEXPOSURE is bit 8; LDR_INPUT_COLOR is bit 6.
    // We feed linear HDR without an exposure texture, with low-res unjittered motion and forward depth.
    internal const uint InitFlags = 1u << 8;
    private readonly XessNative? api;
    private readonly Action<string> log;
    private IUpscalerDevice? device;
    private nint instance, physical, logical;
    private XessContext? context;
    private UpscalerPlan initializedPlan;
    private int color, depth, motion, output;
    private bool ready, firstFrame;
    /// <inheritdoc/>
    public string Id => "xess";
    /// <inheritdoc/>
    public string Name => "XeSS-SR";
    /// <inheritdoc/>
    public bool Active => ready && Unavailable == null;
    /// <inheritdoc/>
    public string? Unavailable { get; private set; }
    /// <inheritdoc/>
    public IDeviceRequirementContributor? Requirements => api == null ? null : this;

    /// <summary>Loads the optional XeSS SR exports and records load failure as unavailability.</summary>
    /// <param name="log">Destination for XeSS-operation failures.</param>
    public XessBackend(Action<string> log)
    {
        this.log = log;
        api = XessNative.TryLoad(out string? error);
        Unavailable = error;
    }
    private bool Check(int result, string operation)
    {
        if (result >= 0) return true; // Positive values are SDK warnings.
        Unavailable = operation + " failed (XeSS " + result + ")";
        log("[VulkanStory] " + Unavailable);
        return false;
    }
    /// <summary>Rejects null or excessive SDK extension lists before reading their entries.</summary>
    private bool CheckExtensionList(uint count, byte** names)
    {
        if (count <= 256 && (count == 0 || names != null)) return true;
        Unavailable = "XeSS returned an invalid Vulkan extension list";
        log("[VulkanStory] " + Unavailable);
        return false;
    }
    /// <inheritdoc/>
    public void ContributeInstanceExtensions(InstanceRequirements requirements)
    {
        if (api == null || Unavailable != null) return;
        uint count = 0, version = 0; byte** names = null;
        if (!Check(api.InstanceExtensions(&count, &names, &version), "XeSS instance requirements")) return;
        if (version > Vk.Version13) { Unavailable = "XeSS requires a newer Vulkan API"; return; }
        if (!CheckExtensionList(count, names)) return;
        for (uint i = 0; i < count; i++)
        {
            string? extension = names[i] == null ? null : Marshal.PtrToStringUTF8((nint)names[i]);
            if (string.IsNullOrWhiteSpace(extension) || !requirements.Request(extension, Name))
            {
                Unavailable = "XeSS requires an unavailable instance extension";
                return;
            }
        }
    }
    /// <inheritdoc/>
    public void ContributeDeviceRequirements(DeviceRequirements requirements)
    {
        if (api == null || Unavailable != null) return;
        instance = requirements.Instance.Handle; physical = requirements.PhysicalDevice.Handle;
        uint count = 0; byte** names = null;
        if (!Check(api.DeviceExtensions(instance, physical, &count, &names), "XeSS device extensions")) return;
        if (!CheckExtensionList(count, names)) return;
        // Probe support before modifying the renderer's feature chain.
        void* probe = null;
        if (!Check(api.DeviceFeatures(instance, physical, &probe), "XeSS device features")) return;
        for (uint i = 0; i < count; i++)
        {
            string? extension = names[i] == null ? null : Marshal.PtrToStringUTF8((nint)names[i]);
            if (string.IsNullOrWhiteSpace(extension) || !requirements.Request(extension, requestedBy: Name))
            {
                Unavailable = "XeSS requires an unavailable device extension";
                return;
            }
        }
    }
    /// <inheritdoc/>
    public void FinalizeDeviceFeatures(DeviceRequirements requirements, void** features)
    {
        if (api == null || Unavailable != null) return;
        try
        {
            Check(InvokeDeviceFeatureNegotiation(features,
                chain => api.DeviceFeatures(instance, physical, chain)), "XeSS feature negotiation");
        }
        catch (Exception e)
        {
            Unavailable = "XeSS feature negotiation failed: " + e.Message;
            log("[VulkanStory] " + Unavailable);
        }
    }

    /// <summary>SDK callback that may modify the supplied Vulkan feature-chain pointer.</summary>
    internal delegate int DeviceFeatureCall(void** features);

    // The SDK may change any node in the supplied pNext chain before returning an
    // error. Save every node we currently put in that chain, not only the core
    // Vulkan 1.2/1.3 nodes, so another provider sees the original feature set.
    /// <summary>Snapshots supported feature-chain nodes, invokes SDK negotiation and restores their bytes when negotiation fails.</summary>
    /// <param name="features">Address of the renderer-owned Vulkan feature-chain head.</param>
    /// <param name="call">SDK negotiation callback, which may mutate the chain.</param>
    /// <returns>The SDK result code; nonnegative results preserve negotiated changes.</returns>
    /// <exception cref="InvalidOperationException">The chain is cyclic, too long or contains an unsupported node type.</exception>
    internal static int InvokeDeviceFeatureNegotiation(void** features, DeviceFeatureCall call)
    {
        void* original = *features;
        var saved = new List<(nint Address, byte[] Data)>();
        var visited = new HashSet<nint>();
        for (BaseOutStructure* node = (BaseOutStructure*)original; node != null; node = node->PNext)
        {
            if (!visited.Add((nint)node) || saved.Count >= 32)
                throw new InvalidOperationException("invalid Vulkan feature chain");
            int size = node->SType switch
            {
                StructureType.PhysicalDeviceFeatures2 => sizeof(PhysicalDeviceFeatures2),
                StructureType.PhysicalDeviceVulkan12Features => sizeof(PhysicalDeviceVulkan12Features),
                StructureType.PhysicalDeviceVulkan13Features => sizeof(PhysicalDeviceVulkan13Features),
                StructureType.PhysicalDeviceFaultFeaturesExt => sizeof(PhysicalDeviceFaultFeaturesEXT),
                StructureType.PhysicalDeviceAntiLagFeaturesAmd => sizeof(PhysicalDeviceAntiLagFeaturesAMD),
                StructureType.PhysicalDeviceColorWriteEnableFeaturesExt => sizeof(PhysicalDeviceColorWriteEnableFeaturesEXT),
                StructureType.PhysicalDeviceExtendedDynamicState3FeaturesExt => sizeof(PhysicalDeviceExtendedDynamicState3FeaturesEXT),
                StructureType.PhysicalDevicePresentIDFeaturesKhr => sizeof(PhysicalDevicePresentIdFeaturesKHR),
                StructureType.PhysicalDeviceSwapchainMaintenance1FeaturesExt => sizeof(PhysicalDeviceSwapchainMaintenance1FeaturesEXT),
                _ => throw new InvalidOperationException("unknown Vulkan feature node: " + node->SType),
            };
            var data = new byte[size];
            Marshal.Copy((nint)node, data, 0, size);
            saved.Add(((nint)node, data));
        }
        bool success = false;
        try
        {
            int result = call(features);
            success = result >= 0;
            return result;
        }
        finally
        {
            // A successful call deliberately keeps XeSS's additions and bits.
            // Restore on error or exception; the caller marks XeSS unavailable.
            if (!success)
            {
                *features = original;
                foreach (var (address, data) in saved)
                    Marshal.Copy(data, 0, address, data.Length);
            }
        }
    }
    /// <inheritdoc/>
    public bool BringUp(IUpscalerDevice target, nint instance, nint physicalDevice, nint logicalDevice)
    {
        if (api == null || Unavailable != null) return false;
        device = target; this.instance = instance; physical = physicalDevice; logical = logicalDevice;
        ready = CreateContext();
        return ready;
    }
    private bool CreateContext()
    {
        nint handle = 0;
        if (!Check(api!.Create(instance, physical, logical, &handle), "xessVKCreateContext")) return false;
        context = new XessContext(api, handle);
        return true;
    }
    /// <summary>Maps renderer quality names to XeSS quality constants; unknown names select Quality.</summary>
    internal static int QualityOf(string? quality) => quality?.ToLowerInvariant() switch
    {
        "ultraperformance" => 100, "performance" => 101, "balanced" => 102,
        "ultraquality" => 104, "ultraqualityplus" => 105, "dlaa" or "native" => 106, _ => 103,
    };
    /// <inheritdoc/>
    public bool TryPlan(int displayWidth, int displayHeight, string quality,
        float lodBiasOffset, out UpscalerPlan plan)
    {
        plan = default;
        if (!Active || displayWidth <= 0 || displayHeight <= 0) return false;
        if (context == null && !CreateContext()) return false;
        XessSize output = new(displayWidth, displayHeight), optimal = default, minimum = default, maximum = default;
        quality = string.IsNullOrWhiteSpace(quality) ? "quality" : quality;
        if (!Check(api!.Resolution(context!.Handle, &output, QualityOf(quality), &optimal, &minimum, &maximum),
            "xessGetOptimalInputResolution")) return false;
        plan = new UpscalerPlan((int)optimal.Width, (int)optimal.Height, displayWidth, displayHeight,
            quality, RecommendedLodBias((int)optimal.Width, displayWidth) * Math.Clamp(lodBiasOffset, 0f, 1f), lodBiasOffset);
        return plan.IsValid;
    }
    // XeSS-SR guide: additional bias = log2(input width / target width).
    /// <summary>Computes the additional XeSS texture LOD bias from positive input and output widths.</summary>
    /// <returns>The base-two logarithm of the input-to-output width ratio.</returns>
    internal static float RecommendedLodBias(int renderWidth, int displayWidth) =>
        MathF.Log2((float)renderWidth / displayWidth);
    /// <inheritdoc/>
    public bool Evaluate(in UpscalerPlan plan, in UpscalerFrame frame, out string? error)
    {
        error = null;
        if (!Active || !plan.IsValid) { error = Unavailable ?? "XeSS is unavailable"; return false; }
        if (initializedPlan != plan)
        {
            // Never reinitialize a context that an in-flight command buffer references.
            if (initializedPlan.IsValid) RetireFeature();
            if (context == null && !CreateContext()) { error = Unavailable; return false; }
            var init = new XessInit { Output = new(plan.DisplayWidth, plan.DisplayHeight),
                Quality = QualityOf(plan.Quality), Flags = InitFlags };
            if (!Check(api!.Init(context!.Handle, &init), "xessVKInit")) { error = Unavailable; return false; }
            color = device!.CreateUpscaleTexture(plan.RenderWidth, plan.RenderHeight, Format.R16G16B16A16Sfloat, true);
            depth = device.CreateUpscaleTexture(plan.RenderWidth, plan.RenderHeight, Format.R32Sfloat, true);
            motion = device.CreateUpscaleTexture(plan.RenderWidth, plan.RenderHeight, Format.R16G16Sfloat, true);
            output = device.CreateUpscaleTexture(plan.DisplayWidth, plan.DisplayHeight, Format.R16G16B16A16Sfloat, true);
            initializedPlan = plan;
            firstFrame = true;
        }
        int result = device!.EvaluateXess(api!, context!.Handle, color, depth, motion, output, frame, firstFrame);
        firstFrame = false;
        if (Check(result, "xessVKExecute")) return true;
        error = Unavailable;
        return false;
    }
    /// <inheritdoc/>
    public void RetireFeature()
    {
        if (context != null) { device!.RetireUpscalerResource(context); context = null; }
        foreach (int texture in new[] { color, depth, motion, output })
            if (texture != 0) device!.DeleteTexture(texture);
        color = depth = motion = output = 0;
        initializedPlan = default;
    }
    /// <inheritdoc/>
    public void Shutdown() { RetireFeature(); ready = false; }
    /// <inheritdoc/>
    public void Dispose() => Shutdown();

    /// <summary>Owns a XeSS SR context released after its GPU references complete.</summary>
    /// <remarks>Negative SDK destruction results retain ownership and make further release terminal.</remarks>
    private sealed class XessContext(XessNative api, nint handle) : IDisposable
    {
        private Exception? releaseFailure;
        public nint Handle { get; private set; } = handle;
        /// <inheritdoc/>
        public void Dispose()
        {
            if (releaseFailure != null)
                throw new InvalidOperationException("XeSS SR release failed; cleanup is terminal.", releaseFailure);
            if (Handle == 0) return;
            int result = api.Destroy(Handle);
            if (result < 0) // Preserve the backend's nonnegative SDK-warning convention.
            {
                releaseFailure = new InvalidOperationException("XeSS SR context destruction failed (" + result + "); ownership retained.");
                throw releaseFailure;
            }
            Handle = 0;
        }
    }
}

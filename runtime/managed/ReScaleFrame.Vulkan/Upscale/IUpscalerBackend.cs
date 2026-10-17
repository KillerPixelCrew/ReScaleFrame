using System;
using VulkanStory.Contracts;

namespace ReScaleFrame.Vulkan;

/// <summary>Renderer texture IDs and matching temporal constants for one reconstruction dispatch.</summary>
/// <param name="Color">Linear scene-color input texture.</param>
/// <param name="Depth">Depth input texture used by the scene's motion producers.</param>
/// <param name="Motion">Renderer motion texture; the backend converts it when its SDK requires another format.</param>
/// <param name="Output">Display-resolution destination texture.</param>
/// <param name="Temporal">Jitter, camera, timing and history-reset data for this frame.</param>
/// <param name="Composition">Borrowed current-frame transparency revealage texture, when present.</param>
/// <param name="HasComposition">Whether this frame rendered transparency into the revealage texture.</param>
/// <param name="MaterialMotionSource">Original scene guide used for pre-composition material-reactive capture; zero when Motion is that same source.</param>
internal readonly record struct UpscalerFrame(
    int Color, int Depth, int Motion, int Output, TemporalProviderFrame Temporal,
    int Composition = 0, bool HasComposition = false, int MaterialMotionSource = 0);

/// <summary>
/// One selectable reconstruction provider. All installed providers prepare before Vulkan device
/// creation, so switching the selected id later needs only a target rebuild and history reset.
/// </summary>
internal interface IUpscalerBackend : IDisposable
{
    /// <summary>Stable settings ID used to select this provider.</summary>
    string Id { get; }
    /// <summary>Whether device bring-up succeeded and no availability failure has been recorded.</summary>
    bool Active { get; }
    /// <summary>Latest provider-unavailability reason, or <see langword="null"/> when none is recorded.</summary>
    string? Unavailable { get; }
    /// <summary>Optional extension and feature contributor to invoke before creating the Vulkan device.</summary>
    IDeviceRequirementContributor? Requirements { get; }
    /// <summary>Attaches the provider to an already created renderer device.</summary>
    /// <param name="device">Renderer owner used for resources, dispatch and GPU-safe retirement.</param>
    /// <param name="instance">Borrowed Vulkan instance handle.</param>
    /// <param name="physicalDevice">Borrowed selected physical-device handle.</param>
    /// <param name="logicalDevice">Borrowed logical-device handle.</param>
    /// <returns>Whether this provider can be used with the selected device.</returns>
    bool BringUp(IUpscalerDevice device, IntPtr instance, IntPtr physicalDevice, IntPtr logicalDevice);
    /// <summary>Resolves the requested output extent and quality into provider input dimensions.</summary>
    /// <param name="displayWidth">Output width in pixels.</param>
    /// <param name="displayHeight">Output height in pixels.</param>
    /// <param name="quality">Settings quality name; interpretation is provider-specific.</param>
    /// <param name="lodBiasOffset">Requested strength of the provider's additional texture LOD bias.</param>
    /// <param name="plan">Resolved plan on success; invalid or default on failure.</param>
    /// <returns>Whether an active provider produced a valid plan.</returns>
    bool TryPlan(int displayWidth, int displayHeight, string quality,
        float lodBiasOffset, out UpscalerPlan plan);
    /// <summary>Records reconstruction for the current frame, creating or replacing plan-dependent resources as needed.</summary>
    /// <param name="plan">Valid plan matching the input and output textures.</param>
    /// <param name="frame">Texture IDs and temporal constants retained through GPU completion.</param>
    /// <param name="error">Provider failure detail, or <see langword="null"/> on success.</param>
    /// <returns>Whether the provider accepted the dispatch; this does not wait for GPU completion.</returns>
    bool Evaluate(in UpscalerPlan plan, in UpscalerFrame frame, out string? error);
    /// <summary>Releases or schedules retirement of plan-dependent resources before reuse or provider switching.</summary>
    /// <remarks>Checked release failures may throw and retain native owners; callers must preserve their lifetime.</remarks>
    void RetireFeature();
    /// <summary>Retires active resources and stops the backend before renderer-device destruction.</summary>
    void Shutdown();
}

/// <summary>The DLSS-SR implementation of the shared provider contract.</summary>
internal sealed class DlssBackend(DlssUpscaler host) : IUpscalerBackend
{
    /// <inheritdoc/>
    public string Id => "dlss";
    /// <inheritdoc/>
    public bool Active => host.Active;
    /// <inheritdoc/>
    public string? Unavailable => host.Unavailable;
    /// <inheritdoc/>
    public IDeviceRequirementContributor? Requirements => host.Requirements;

    /// <inheritdoc/>
    public bool BringUp(IUpscalerDevice device, IntPtr instance, IntPtr physicalDevice, IntPtr logicalDevice) =>
        host.BringUp(device, instance, physicalDevice, logicalDevice);

    /// <inheritdoc/>
    public bool TryPlan(int displayWidth, int displayHeight, string quality,
        float lodBiasOffset, out UpscalerPlan plan)
    {
        plan = default;
        if (!host.TryPlan(displayWidth, displayHeight, quality, lodBiasOffset,
                out UpscalePlan vendor)) return false;
        plan = new UpscalerPlan(vendor.RenderWidth, vendor.RenderHeight,
            vendor.DisplayWidth, vendor.DisplayHeight, quality, null, lodBiasOffset);
        return plan.IsValid;
    }

    /// <inheritdoc/>
    public bool Evaluate(in UpscalerPlan plan, in UpscalerFrame frame, out string? error)
    {
        error = null;
        var vendor = new UpscalePlan(plan.RenderWidth, plan.RenderHeight,
            plan.DisplayWidth, plan.DisplayHeight, DlssUpscaler.QualityOf(plan.Quality),
            plan.LodBiasOffset);
        if (!host.EnsureFeature(vendor))
        {
            error = host.Unavailable ?? "DLSS feature creation failed";
            return false;
        }
        NgxResult result = host.Evaluate(frame.Color, frame.Depth, frame.Motion, frame.Output,
            NgxDlssEvaluation.FromTemporalFrame(frame.Temporal));
        if (result == NgxResult.Success) return true;
        error = "NVSDK_NGX_VULKAN_EvaluateFeature: " + NgxInterop.Describe(result);
        return false;
    }

    /// <inheritdoc/>
    public void RetireFeature() => host.RetireFeature();
    /// <inheritdoc/>
    public void Shutdown() => host.Shutdown();
    /// <inheritdoc/>
    public void Dispose() => host.Dispose();
}

using System;
using System.Collections.Generic;
using System.IO;
using VulkanStory.Contracts;

namespace ReScaleFrame.Vulkan;

/// <summary>
/// What an upscaler asks the engine to render at, answered by the vendor's own
/// query rather than by a hard-coded ratio per preset.
///
/// Everything downstream is derived here so no second place can invent its own
/// number: the jitter phase count (the temporal contract's
/// <c>max(1, ceil(8 * upscale^2))</c>, §2, with the upscale factor this plan
/// really uses), and the texture LOD bias every vendor SDK recommends
/// (<c>log2(render/display) - 1</c>).
/// </summary>
internal readonly record struct UpscalePlan(
    int RenderWidth, int RenderHeight, int DisplayWidth, int DisplayHeight,
    NgxPerfQuality Quality, float LodBiasOffset = 1f)
{
    /// <summary>Whether this plan describes a usable render target pair.</summary>
    public bool IsValid =>
        RenderWidth > 0 && RenderHeight > 0 && DisplayWidth > 0 && DisplayHeight > 0 &&
        RenderWidth <= DisplayWidth && RenderHeight <= DisplayHeight;

    /// <summary>Render pixels per display pixel on the X axis; 1 for DLAA.</summary>
    public float RenderScale => DisplayWidth <= 0 ? 1f : (float)RenderWidth / DisplayWidth;

    /// <summary>The bias the SDK recommends for this ratio; 0 when there is no upscale.</summary>
    public float LodBias => UpscalerPlan.RecommendedLodBias(RenderWidth, DisplayWidth, LodBiasOffset);

    /// <summary>
    /// The contract's phase count for this plan's real upscale factor, so the
    /// sequence length follows the vendor's render size instead of the config's
    /// render scale (temporal contract §7.2 explicitly allows taking it from the
    /// SDK, and DLSS asks for 8 * upscale^2 like the others).
    /// </summary>
    public int JitterPhaseCount => Math.Max(1, (int)Math.Ceiling(
        8.0 * (RenderScale > 0f ? 1.0 / RenderScale : 1.0) * (RenderScale > 0f ? 1.0 / RenderScale : 1.0)));

    public NgxDlssSettings ToSettings() => new(
        (uint)RenderWidth, (uint)RenderHeight, (uint)DisplayWidth, (uint)DisplayHeight,
        Quality, NgxDlssSettings.ContractFlags);

    /// <summary>
    /// One line, in a fixed format. Invariant culture: this string is a log line and
    /// the settings tab's plan readout, so its numbers must read the same on every
    /// machine rather than picking up the user's decimal separator.
    /// </summary>
    public override string ToString() =>
        RenderWidth + "x" + RenderHeight + " -> " + DisplayWidth + "x" + DisplayHeight +
        " " + Quality + " (scale " +
        RenderScale.ToString("0.###", System.Globalization.CultureInfo.InvariantCulture) +
        ", lod bias " +
        LodBias.ToString("0.##", System.Globalization.CultureInfo.InvariantCulture) +
        ", " + JitterPhaseCount + " jitter phases)";
}

/// <summary>
/// DLSS Super Resolution as the client's upscaler slot: the piece between the
/// selected upscaler state and the device seam
/// (<c>IUpscalerDevice.CreateDlssFeature</c> / <c>EvaluateDlss</c>).
///
/// It owns three things the frame must not have to think about:
/// <list type="number">
/// <item><b>Availability.</b> Every way DLSS can be absent - the setting is off,
/// the native shim is missing, the driver library is not installed, no NGX
/// feature libraries, a non-NVIDIA GPU, an NGX that refuses to initialise, a
/// feature that refuses to create - ends in exactly the same place:
/// <see cref="Active" /> false, one line logged, and
/// <see cref="IUpscalerRuntimeState.DisableAtRuntime" /> so the rest of the client runs
/// the pre-DLSS chain for the session. Nothing here throws and nothing aborts.</item>
/// <item><b>The plan.</b> The render size and the preset come from the SDK's own
/// optimal-settings query for the display size, never from a table here.</item>
/// <item><b>Lifetime.</b> One feature per (render size, display size, preset);
/// a change retires the old one onto the frame timeline and creates a new one,
/// so a resize or a preset change cannot leak features - and none of them touches
/// NGX itself. The device initializer owns its lifetime: either Streamline or
/// <see cref="NgxLifetime" /> for standalone operation. A settings change never
/// ends it; each consumer releases its features before the owner shuts down
/// the device's NGX instance.</item>
/// </list>
/// </summary>
internal sealed class DlssUpscaler : IDisposable
{
    private readonly IUpscalerRuntimeState _state;
    private readonly Action<string> _log;
    private NgxSession? _session;
    private bool _sessionAdopted;
    private bool _ownsNgxLifetime;
    private IUpscalerDevice? _device;
    private IntPtr _vkDevice;
    private NgxDlssFeature? _feature;
    private bool _disposed;


    public DlssUpscaler(IUpscalerRuntimeState state, Action<string>? log = null)
    {
        _state = state ?? throw new ArgumentNullException(nameof(state));
        _log = log ?? (_ => { });
    }

    /// <summary>Whether the config asks for DLSS at all, before anything is loaded.</summary>
    public bool Requested => _state.DlssRequested;

    /// <summary>Why the upscaler is not running, or null while it is.</summary>
    public string? Unavailable { get; private set; } = "not brought up";

    /// <summary>Whether DLSS is up on a device and can be planned and evaluated.</summary>
    public bool Active => !_disposed && _session != null && _device != null && Unavailable == null;

    /// <summary>The plan the live feature serves, or default when there is none.</summary>
    public UpscalePlan Plan { get; private set; }

    /// <summary>The S1 requirement contributor, created with the session; null when there is none.</summary>
    public NgxDeviceRequirements? Requirements { get; private set; }

    /// <summary>How many features this host has created, and how many it has retired. Never leaks: the two agree once shut down.</summary>
    public int FeaturesCreated { get; private set; }

    public int FeaturesRetired { get; private set; }

    /// <summary>The live feature, for tests that want to see it change across a resize.</summary>
    internal NgxDlssFeature? Feature => _feature;

    // ------------------------------------------------------------ bring-up

    /// <summary>
    /// The half that has to happen <i>before</i> the Vulkan device exists: NGX's
    /// instance and device extensions are requested at device creation, so the
    /// session and its requirement contributor are built first and handed to the
    /// context options.
    ///
    /// Answers null when DLSS is not wanted or cannot possibly work here, having
    /// logged the one line and stood the setting down. A null here is not an
    /// error: the client carries on with the chain it has.
    /// </summary>
    public static DlssUpscaler? TryPrepare(IUpscalerRuntimeState state,
        string applicationDataPath, Action<string>? log = null,
        bool prepareForSwitching = false)
    {
        var host = new DlssUpscaler(state, log);
        if (!host.Requested && !prepareForSwitching)
        {
            host.Unavailable = "the Upscaler setting is not \"dlss\"";
            return null;
        }
        if (NgxLifetime.Spent)
        {
            if (host.Requested) host.Fail("NGX was already shut down in this process and allows exactly one lifetime");
            return null;
        }

        string? diagnosis = Diagnose();
        if (diagnosis != null)
        {
            if (host.Requested) host.Fail(diagnosis);
            return null;
        }

        IReadOnlyList<string> paths = NgxSession.FindFeaturePaths();
        string dataPath = applicationDataPath;
        try
        {
            if (!string.IsNullOrEmpty(dataPath)) Directory.CreateDirectory(dataPath);
        }
        catch (IOException)
        {
            // NGX only writes logs and caches there; an unwritable path is not
            // worth refusing the feature over.
        }
        catch (UnauthorizedAccessException)
        {
        }

        host._session = new NgxSession("1.0.0", dataPath, paths);
        host.Requirements = new NgxDeviceRequirements(
            host._session, new[] { NgxFeature.SuperSampling, NgxFeature.FrameGeneration },
            line => host._log("[VulkanStory] NGX: " + line));
        host.Unavailable = "the device has not been brought up yet";
        return host;
    }

    /// <summary>
    /// Once the device exists, borrows an existing NGX instance or initializes
    /// the standalone device instance.
    /// False means the upscaler is not available and the setting has been stood
    /// down; the caller carries on with the old chain.
    /// </summary>
    public bool BringUp(IUpscalerDevice device, IntPtr instance, IntPtr physicalDevice, IntPtr vkDevice)
    {
        if (_disposed) return Fail("the upscaler host was already shut down");
        if (NgxLifetime.Spent)
        {
            return Fail("NGX was already shut down in this process and allows exactly one lifetime");
        }
        if (instance == IntPtr.Zero || physicalDevice == IntPtr.Zero || vkDevice == IntPtr.Zero)
        {
            return Fail("the Vulkan device did not publish the handles NGX needs");
        }
        if (_session == null || device == null)
        {
            return Fail("there is no NGX session to bring up on this device");
        }

        // Init is once per device, not once per consumer. Streamline's common
        // plugin may already own this device even if DLSS-G startup later failed.
        // Probe the public runtime before taking ownership; function-bind failure
        // alone does not establish that the device has no NGX lifetime.
        IntPtr capabilityHandle = IntPtr.Zero;
        NgxResult capabilities = NgxResult.FailNotInitialized;
        if (device.HasExternalNgxInitializer)
        {
            capabilities = NgxInterop.GetCapabilityParameters(out capabilityHandle);
            if (capabilities == NgxResult.Success && capabilityHandle != IntPtr.Zero)
                _log("[VulkanStory] DLSS: borrowing Streamline's NGX device lifetime; host Init and Shutdown omitted.");
            else if (capabilities != NgxResult.FailNotInitialized || capabilityHandle != IntPtr.Zero)
                return Fail("External NGX ownership cannot be established: " + NgxInterop.Describe(capabilities));
        }
        if (capabilities == NgxResult.FailNotInitialized)
        {
            NgxLifetimeOutcome brought = NgxLifetime.Initialize(
                _session, instance, physicalDevice, vkDevice, out NgxResult initialized);
            if (brought != NgxLifetimeOutcome.Done)
                return Fail(brought == NgxLifetimeOutcome.NotInitialized
                    ? "NVSDK_NGX_VULKAN_Init_ProjectID: " + NgxInterop.Describe(initialized)
                    : "NGX device lifetime could not be acquired: " + brought);
            _ownsNgxLifetime = true;
            capabilities = NgxInterop.GetCapabilityParameters(out capabilityHandle);
        }
        if (capabilities != NgxResult.Success || capabilityHandle == IntPtr.Zero)
        {
            return Fail("NVSDK_NGX_VULKAN_GetCapabilityParameters: " + NgxInterop.Describe(capabilities));
        }
        try
        {
            NgxResult availableStatus = new NgxParameters(capabilityHandle).GetUInt(
                NgxParameterNames.SuperSamplingAvailable, out uint available);
            if (availableStatus != NgxResult.Success || available == 0)
            {
                return Fail("NGX reports SuperSampling.Available=" + available +
                    " (" + NgxInterop.Describe(availableStatus) + ")");
            }
        }
        finally
        {
            NgxInterop.DestroyParameters(capabilityHandle);
        }

        _device = device;
        _vkDevice = vkDevice;
        Unavailable = null;
        _log("[VulkanStory] DLSS Super Resolution is available and will upscale the frame.");
        return true;
    }

    /// <summary>
    /// Test seam: adopts an NGX session somebody else brought up - the
    /// one-per-process test runtime - without taking over its lifetime. Init and
    /// Shutdown stay the owner's business; everything else (plans, features,
    /// evaluates) is this host's, exactly as in the client. Refused once the host
    /// holds a session of its own, whose shutdown it must not give up.
    /// </summary>
    internal bool AdoptSession(NgxSession session, IUpscalerDevice device, IntPtr vkDevice)
    {
        if (_disposed || (_session != null && !_sessionAdopted)) return false;
        _session = session;
        _sessionAdopted = true;
        _device = device;
        _vkDevice = vkDevice;
        Unavailable = null;
        return true;
    }

    // ---------------------------------------------------------------- plan

    /// <summary>
    /// What to render at for this display size, from the SDK's own query. False
    /// leaves <paramref name="plan" /> at its default and, when the query itself
    /// failed, stands the upscaler down.
    /// </summary>
    public bool TryPlan(int displayWidth, int displayHeight, string preset,
        float lodBiasOffset, out UpscalePlan plan)
    {
        plan = default;
        if (!Active || displayWidth <= 0 || displayHeight <= 0) return false;

        NgxPerfQuality quality = QualityOf(preset);
        NgxResult capabilities = NgxInterop.GetCapabilityParameters(out IntPtr handle);
        if (capabilities != NgxResult.Success || handle == IntPtr.Zero)
        {
            return Fail("NVSDK_NGX_VULKAN_GetCapabilityParameters: " + NgxInterop.Describe(capabilities));
        }

        try
        {
            NgxResult result = NgxSession.OptimalSettings(
                new NgxParameters(handle), (uint)displayWidth, (uint)displayHeight, quality,
                out NgxOptimalSettings settings);
            if (result != NgxResult.Success || settings.OptimalWidth == 0 || settings.OptimalHeight == 0)
            {
                var probe = new NgxParameters(handle);
                NgxResult widthStatus = probe.GetUInt(NgxParameterNames.Width, out uint widthValue);
                NgxResult heightStatus = probe.GetUInt(NgxParameterNames.Height, out uint heightValue);
                NgxResult qualityStatus = probe.GetInt(NgxParameterNames.PerfQualityValue, out int qualityValue);
                return Fail("the optimal-settings query for " + displayWidth + "x" + displayHeight +
                    " " + quality + " answered " + NgxInterop.Describe(result) +
                    "; width=" + widthValue + " (" + NgxInterop.Describe(widthStatus) +
                    "), height=" + heightValue + " (" + NgxInterop.Describe(heightStatus) +
                    "), quality=" + qualityValue + " (" + NgxInterop.Describe(qualityStatus) + ")");
            }

            plan = new UpscalePlan(
                (int)settings.OptimalWidth, (int)settings.OptimalHeight,
                displayWidth, displayHeight, quality, lodBiasOffset);
            return plan.IsValid;
        }
        finally
        {
            NgxInterop.DestroyParameters(handle);
        }
    }

    /// <summary>
    /// The preset names the setting may hold, mapped onto NGX's own enum.
    /// Anything else is Quality, which is what the config parser already
    /// guarantees; this is the second line of defence, not the first.
    /// </summary>
    public static NgxPerfQuality QualityOf(string? preset) =>
        string.Equals(preset, "dlaa", StringComparison.OrdinalIgnoreCase) ? NgxPerfQuality.Dlaa :
        string.Equals(preset, "balanced", StringComparison.OrdinalIgnoreCase) ? NgxPerfQuality.Balanced :
        string.Equals(preset, "performance", StringComparison.OrdinalIgnoreCase) ? NgxPerfQuality.MaxPerf :
        string.Equals(preset, "ultraperformance", StringComparison.OrdinalIgnoreCase) ? NgxPerfQuality.UltraPerformance :
        NgxPerfQuality.MaxQuality;

    // ------------------------------------------------------------ lifetime

    /// <summary>
    /// Makes sure a feature exists for <paramref name="plan" />, creating one on
    /// this frame's command buffer and retiring any feature that was created for
    /// a different plan. Must be called inside an open frame: NGX records
    /// initialisation work into the command buffer it is given.
    ///
    /// False means no feature - the caller runs the frame without the upscale.
    /// A creation failure stands the upscaler down, because a driver that refuses
    /// to create the feature will refuse again next frame.
    /// </summary>
    public bool EnsureFeature(in UpscalePlan plan)
    {
        if (!Active || _device == null || !plan.IsValid) return false;

        NgxDlssSettings settings = plan.ToSettings() with
        {
            RenderPreset = _state.DlssPreset switch { "j" => 10, "k" => 11, "l" => 12, "m" => 13, _ => 0u },
        };
        if (_feature != null && _feature.Matches(settings))
        {
            return true;
        }

        // A plan change is a new feature, never a reconfigured one: NGX sizes its
        // internal buffers at creation. The old one goes onto the frame timeline,
        // because an evaluate recorded this frame may still name it.
        RetireFeature();

        NgxResult created = _device.CreateDlssFeature(settings, out NgxDlssFeature? feature);
        if (created != NgxResult.Success || feature == null)
        {
            return Fail("NVSDK_NGX_VULKAN_CreateFeature1 " + settings + ": " + NgxInterop.Describe(created));
        }

        _feature = feature;
        FeaturesCreated++;
        // The lifetime owner counts live features, because it is the thing that has
        // to refuse a shutdown while one of them could still name NGX state.
        NgxLifetime.FeatureCreated();
        Plan = plan;
        // The LOD bias belongs to the plan that was really created, not to the one
        // that was asked for, so the samplers follow a feature that fell back.
        _state.SetActivePlan(plan.RenderScale, plan.LodBias);
        _log("[VulkanStory] DLSS feature created: " + plan);
        return true;
    }

    /// <summary>
    /// Runs the upscale for this frame. The caller has already made sure the
    /// feature matches the plan (<see cref="EnsureFeature" />) and that the four
    /// textures are the render-resolution colour, depth and motion and the
    /// display-resolution output.
    /// </summary>
    public NgxResult Evaluate(
        int colorTexture, int depthTexture, int motionTexture, int outputTexture, in NgxDlssEvaluation frame)
    {
        if (!Active || _device == null || _feature == null) return NgxResult.FailFeatureNotFound;
        return _device.EvaluateDlss(_feature, colorTexture, depthTexture, motionTexture, outputTexture, frame);
    }

    /// <summary>
    /// Retires the live feature onto the frame timeline. Never destroys it
    /// inline: an evaluate recorded this frame is still in flight.
    /// </summary>
    public void RetireFeature()
    {
        if (_feature == null) return;
        NgxDlssFeature feature = _feature;
        _feature = null;
        Plan = default;
        FeaturesRetired++;
        NgxLifetime.FeatureRetired();
        _state.ClearActivePlan();
        if (_device != null) _device.RetireDlssFeature(feature);
        else feature.Dispose();
    }

    /// <summary>
    /// Retires this consumer's features and drains the frame timeline. An owned
    /// standalone instance then shuts down through <see cref="NgxLifetime.ShutDown" />.
    /// A borrowed instance stays initialized until its external owner releases
    /// its remaining features and shuts down, before Vulkan device destruction.
    ///
    /// <para>This is the path that ends our standalone NGX lifetime. A settings
    /// change - the upscaler switched off, a different preset, a runtime stand-down -
    /// never comes here: it retires the feature and leaves NGX up, because the
    /// process cannot bring it back afterwards.</para>
    ///
    /// A host that adopted somebody else's session (<see cref="AdoptSession" />)
    /// retires its feature and drains, but shuts nothing down; the owner does that.
    /// </summary>
    public void Shutdown()
    {
        if (_disposed) return;

        IUpscalerDevice? device = _device;
        if (_sessionAdopted)
        {
            // An adopted session: the feature is this host's, the lifetime is not.
            RetireFeature();
            device?.DrainDeferredDeletions();
        }
        else if (_session != null)
        {
            // Ownership is acquired before capabilities or feature creation can
            // fail. Those failures must still release an owned device instance.
            if (_ownsNgxLifetime)
            {
                NgxLifetimeOutcome outcome = NgxLifetime.ShutDown(RetireFeature, device != null ? device.DrainDeferredDeletions : null, _log);
                if (outcome is NgxLifetimeOutcome.ShutdownFailed or NgxLifetimeOutcome.FeatureStillLive)
                    throw new InvalidOperationException("NGX shutdown did not release its owners: " + outcome);
            }
            else
            {
                // Release only our SR handles. The external owner ends NGX after
                // its own feature owners have also released their resources.
                RetireFeature();
                device?.DrainDeferredDeletions();
            }
            _session.Dispose();
        }

        _session = null;
        _sessionAdopted = false;
        _ownsNgxLifetime = false;
        _device = null;
        _vkDevice = IntPtr.Zero;
        Unavailable = "shut down";
        _state.ClearActivePlan();
        _disposed = true;
    }

    /// <inheritdoc/>
    public void Dispose() => Shutdown();

    // --------------------------------------------------------------- detail

    /// <summary>
    /// Everything the environment has to provide before NGX can be brought up at
    /// all, each with the sentence a user can act on. Null means "nothing in the
    /// way".
    /// </summary>
    private static string? Diagnose()
    {
        if (!OperatingSystem.IsLinux() && !OperatingSystem.IsWindows())
        {
            return "DLSS is reached through the NVIDIA driver library, which this platform does not have";
        }
        if (!NgxShim.IsAvailable)
        {
            return "the NGX shim is not loadable: " + NgxShim.Diagnosis;
        }
        if (!NgxInterop.IsDriverLibraryPresent())
        {
            return "the NVIDIA driver library " + NgxInterop.RuntimeLibraryName +
                " is not loadable: " + NgxShim.LastLoadError;
        }
        NgxResult runtime = NgxShim.LoadRuntime();
        if (!NgxInterop.Succeeded(runtime))
        {
            return "the NGX runtime did not load: " + NgxInterop.Describe(runtime) + " " + NgxShim.LastLoadError;
        }
        if (NgxSession.FindFeaturePaths().Count == 0)
        {
            return "no NGX feature libraries were found; set " + NgxSession.FeaturePathVariable +
                " to the directory holding " +
                (OperatingSystem.IsWindows() ? "nvngx_dlss.dll" : "libnvidia-ngx-dlss.so.*");
        }
        return null;
    }

    /// <summary>
    /// The single failure path: one line in the log, the setting stood down for
    /// the session, and false to the caller. Always false, so a caller can
    /// <c>return Fail(...)</c>.
    /// </summary>
    private bool Fail(string reason)
    {
        Unavailable = reason;
        bool first = Requested && _state.DisableAtRuntime();
        if (first)
        {
            _log("[VulkanStory] DLSS is unavailable (" + reason + "); rendering without an upscaler.");
        }
        return false;
    }
}

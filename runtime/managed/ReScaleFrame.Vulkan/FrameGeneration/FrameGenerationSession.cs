using Silk.NET.Vulkan;

namespace ReScaleFrame.Vulkan;

/// <summary>Associates one DLSS-G state-query result with the latency-frame identity at which it was observed.</summary>
internal readonly record struct DlssQueryObservation(ulong QueryFrameId, int Result,
    StreamlineFrameGenerationState? State);

// Direct host migration from the original Optimum VulkanClientPlatform.FrameGeneration.cs.
// Baseline: 386e0d05386d0b228b439d09aeca851428f7bbf3. SDK wrappers,
// upright image formats, camera conversion and presentation ownership are retained.
/// <summary>Coordinates session frame-generation eligibility, matching scene inputs and effective provider state around presentation.</summary>
internal sealed class FrameGenerationSession(IFrameGenerationDevice device, IFrameGenerationSettings settings,
    Action<string> notification, Action<string> error,
    Action requestTemporalReset) : IDisposable
{
    private readonly int ownerThread = Environment.CurrentManagedThreadId;
    private string activeProvider = "off";
    private Fsr3FrameGeneration? fsr3;
    private ulong renderedFrames;
    private uint actualDlssPresents, configuredDlssCount;
    private uint? lastReportedDlssGeneratedLimit;
    private int motionRg, motionWidth, motionHeight;
    private int fsrScene, fsrUi, fsrDepth, fsrMotion, fsrGenerated;
    private int dlssScene, dlssUi, dlssDepth, dlssMotion;
    private int dlssWidth, dlssHeight, dlssRenderWidth, dlssRenderHeight;
    private double xessCameraX, xessCameraY, xessCameraZ;
    private readonly Dictionary<string, string> failed = new(StringComparer.OrdinalIgnoreCase);
    private readonly HashSet<string> waits = new(StringComparer.Ordinal);
    private bool disposed;
    private string preparationStatus = "off";
    /// <summary>Wait reason that <see cref="preparationStatus" /> currently describes, or null for any other status.</summary>
    private string? waitStatusReason;
    /// <summary>Set by every frame that skipped preparation; the next prepared frame resets provider history across the gap.</summary>
    private bool historyResetPending;

    internal string RequestedProvider => settings.Provider;
    internal string EffectiveProvider => failed.ContainsKey(activeProvider) ? "off" : activeProvider;
    // Prepared describes current inputs, not actual SDK interpolation/presentation.
    /// <summary>Whether current frame inputs were prepared; this does not prove SDK interpolation or physical presentation.</summary>
    internal bool PreparedThisFrame { get; private set; }
    /// <summary>
    /// Whether this frame's inputs passed every provider check up to enabling generation, whether
    /// generation then ran or a composition hold stopped it there.
    /// </summary>
    internal bool ReadyToGenerate { get; private set; }
    /// <summary>
    /// Whether this frame waited on a renderer configuration (VSync, output size) that elapsed time
    /// alone does not resolve, so a composition gate must not wait for generation.
    /// </summary>
    internal bool ConfigurationBlocked { get; private set; }
    /// <summary>Wait reason recorded for frames a composition hold stops just before generation.</summary>
    private const string HoldReason = "world composition readiness";
    internal string PreparationStatus => PreparedThisFrame ? "current inputs prepared" : preparationStatus;
    internal uint ConfiguredDlssGeneratedFrames => configuredDlssCount;
    // Configuration after this frame's preparation/presentation handoff;
    // this does not report how many images reached the display.
    /// <summary>Configured generated-image count after current preparation, or zero while unprepared; total multiplier is this count plus one.</summary>
    internal uint ConfiguredGeneratedFrames => !PreparedThisFrame ? 0 : EffectiveProvider switch
    {
        "dlss" => configuredDlssCount,
        "fsr3" => 1,
        "xess" => device.XessConfiguredGeneratedFrames,
        _ => 0,
    };
    /// <summary>Last accumulated DLSS-G SDK-reported presentation count; physical scanout is a separate observation.</summary>
    internal uint ActualDlssPresents => actualDlssPresents;
    /// <summary>Most recent provider-reported generated-image limit, or null when the selected provider has not reported it.</summary>
    internal uint? LastReportedSdkGeneratedLimit => RequestedProvider switch
    {
        "dlss" => lastReportedDlssGeneratedLimit,
        "xess" => device.XessLastReportedGeneratedLimit,
        _ => null,
    };
    internal StreamlineFrameGenerationState? LastDlssState { get; private set; }
    internal int? LastDlssStateResult { get; private set; }
    /// <summary>DLSS-G query receipt for the current harness frame, including query identity and return code.</summary>
    internal DlssQueryObservation? HeadlessDlssQueryObservation { get; private set; }
    internal string? Unavailable(string provider) => failed.GetValueOrDefault(provider);
    internal uint? MaximumMultiplier(string provider) => provider switch
    {
        "fsr3" => 2,
        "dlss" => lastReportedDlssGeneratedLimit is uint dlss ? dlss + 1 : null,
        "xess" => device.XessLastReportedGeneratedLimit is uint xess ? xess + 1 : null,
        _ => null,
    };

    /// <summary>Prepares the selected frame-generation provider using the completed HUD-free scene and matching temporal frame.</summary>
    /// <param name="inputs">Borrowed current-frame targets and synchronous scene producers.</param>
    /// <param name="frame">Snapshot whose frame identity must match the device latency frame.</param>
    /// <param name="hold">
    /// True while the session's composition gate presents a held image: every readiness check,
    /// DLSS-G tagging and its state query still run, but the provider stays paused instead of
    /// enabling, evaluating or preparing generation, so no generated image of the hidden world is
    /// presented around the held one. <see cref="ReadyToGenerate" /> reports whether it would have.
    /// </param>
    /// <remarks>Availability and incomplete inputs keep generation inactive for the frame; provider preparation status records the reason.</remarks>
    internal void Generate(in FrameGenerationFrame frame, IFrameGenerationInputs inputs, bool hold = false)
    {
        RequireOwner();
        HeadlessDlssQueryObservation = null;
        PreparedThisFrame = false;
        ReadyToGenerate = ConfigurationBlocked = false;
        string provider = RequestedProvider;
        if (provider != activeProvider)
        {
            Reset();
            activeProvider = provider;
            LastDlssState = null;
            LastDlssStateResult = null;
            SetStatus(provider == "off" ? "off" : "selected " + provider);
            device.SetFrameGenerationPresentation(provider);
            failed.Remove(provider);
            waits.Clear();
            requestTemporalReset();
            notification("VulkanStory: frame generation provider selected: " + provider);
            if (provider == "fsr3" && settings.Multiplier > 2)
                notification("VulkanStory: FSR 3 Vulkan supports 2×; the requested multiplier applies to multi-frame providers.");
        }
        device.FrameGenerationMultiplier = settings.Multiplier;
        if (provider == "off" || failed.ContainsKey(provider)) return;
        // Menu/loading/pause and missing current producers never reuse old tags.
        if (!frame.CanGenerate || !frame.MotionValid || !frame.HasCamera ||
            frame.FrameId != device.LatencyFrameId)
        { Wait("complete current world camera and motion inputs"); return; }
        if (provider == "dlss" && !device.StreamlineFrameGenerationAvailable)
        { Disable(provider, "Streamline DLSS-G and Reflex are unavailable"); return; }
        if (provider == "dlss" && !device.StreamlineFrameGenerationReady)
        { Wait("the Streamline swapchain rebuild"); return; }
        if (provider == "fsr3" && device.Fsr3ProxyFailure is string fsrFailure)
        { Disable(provider, fsrFailure); return; }
        if (provider == "fsr3" && !device.Fsr3DirectPresentation && !device.Fsr3ProxyReady)
        { Wait("the FidelityFX swapchain rebuild"); return; }
        if (provider == "xess" && device.XessProxyFailure is string xessFailure)
        { Disable(provider, xessFailure); return; }
        // Mailbox presentation or a parked swapchain persists until the user changes it.
        if (provider is not ("dlss" or "xess") && !device.CanPresentGeneratedFrame)
        { ConfigurationWait("a presentable frame-generation-compatible swapchain"); return; }
        if (provider is not ("fsr3" or "dlss" or "xess"))
        { Disable(provider, "unsupported presentation path"); return; }

        int motion = inputs.MotionAttachment;
        FrameGenerationTarget primary = inputs.Primary;
        FrameGenerationTarget output = inputs.Output;
        if (motion < 0 || primary.ColorCount <= motion || primary.DepthTextureId <= 0)
        { NoteWait("primary depth and motion images"); Reset(); return; }
        if (provider == "fsr3" && !output.HasColor)
        { Wait("the FSR3 interpolation output image"); return; }
        FrameGenerationTarget scene = inputs.Scene;
        FrameGenerationTarget ui = inputs.Ui;
        if (!scene.HasColor || !ui.HasColor || ui.Width != scene.Width || ui.Height != scene.Height ||
            (provider == "fsr3" && (scene.Width != output.Width || scene.Height != output.Height)))
        { Wait("matching HUD-less scene and UI images"); return; }
        if (!inputs.TryGetCamera(scene.Width, scene.Height, out NgxFrameGenerationCamera camera))
        { Wait("valid world camera matrices"); return; }

        try
        {
            if (!inputs.TryPrepareMotion(out int preparedMotion, out string motionReason))
            { Wait(motionReason); return; }
            if (provider == "dlss") GenerateDlss(primary, scene, ui, preparedMotion, frame, camera, hold);
            else if (provider == "xess") GenerateXess(inputs, primary, scene, ui, preparedMotion, frame, hold);
            else GenerateFsr3(inputs, primary, scene, ui, output, preparedMotion, frame, hold);
        }
        catch (Exception failure) { Disable(provider, "frame preparation threw: " + failure.Message); }
    }

    private void GenerateFsr3(IFrameGenerationInputs inputs, FrameGenerationTarget primary, FrameGenerationTarget scene, FrameGenerationTarget ui,
        FrameGenerationTarget output, int motionTexture, in FrameGenerationFrame frame, bool hold)
    {
        if (fsr3 == null || fsr3.Width != (uint)output.Width || fsr3.Height != (uint)output.Height ||
            motionWidth != primary.Width || motionHeight != primary.Height)
        {
            Reset();
            int create = Fsr3FrameGeneration.Create(device, (uint)output.Width, (uint)output.Height, out fsr3);
            if (create != 0 || fsr3 == null)
            { Disable("fsr3", "FidelityFX context creation failed (" + create + ")"); return; }
            motionRg = device.CreateUpscaleTexture(primary.Width, primary.Height, Format.R16G16Sfloat, storage: false);
            motionWidth = primary.Width; motionHeight = primary.Height;
            fsrScene = device.CreateUpscaleTexture(output.Width, output.Height, Format.R8G8B8A8Unorm, storage: false);
            fsrUi = device.CreateUpscaleTexture(output.Width, output.Height, Format.R8G8B8A8Unorm, storage: false);
            fsrDepth = device.CreateUpscaleTexture(primary.Width, primary.Height, Format.D32Sfloat, storage: false);
            fsrMotion = device.CreateUpscaleTexture(primary.Width, primary.Height, Format.R16G16Sfloat, storage: false);
            if (device.Fsr3DirectPresentation)
                fsrGenerated = device.CreateUpscaleTexture(output.Width, output.Height, Format.R8G8B8A8Unorm, storage: true);
        }
        // The context and inputs exist; interpolation and the generated present wait for the gate.
        ReadyToGenerate = true;
        if (hold) { Wait(HoldReason); return; }
        int hudless = scene.Color;
        int result = fsr3.Evaluate(device, hudless, primary.DepthTextureId, motionTexture,
            motionRg, hudless, ui.Color, fsrScene, fsrUi, fsrDepth, fsrMotion,
            fsrGenerated, output.Color, frame.Provider);
        if (result != 0)
        { Disable("fsr3", "FidelityFX frame interpolation preparation failed (" + result + ")"); return; }
        if (device.Fsr3DirectPresentation &&
            (!inputs.ComposeGeneratedFrameUi() ||
             !device.QueueGeneratedFrameForPresent(output.Color)))
        { Wait("generated-frame UI composition and presentation"); return; }
        // Native presentation owns the generated image, UI and frame timeline
        // on every device, including devices with additional hardware queues.
        // Fsr3FrameGeneration resets its own history after every Disable in Wait.
        historyResetPending = false;
        PreparedThisFrame = true;
    }

    private void GenerateDlss(FrameGenerationTarget primary, FrameGenerationTarget scene, FrameGenerationTarget ui,
        int motionTexture, in FrameGenerationFrame frame, in NgxFrameGenerationCamera camera, bool hold)
    {
        if (dlssScene == 0 || dlssWidth != scene.Width || dlssHeight != scene.Height ||
            dlssRenderWidth != primary.Width || dlssRenderHeight != primary.Height)
        {
            Reset();
            dlssWidth = scene.Width; dlssHeight = scene.Height;
            dlssRenderWidth = primary.Width; dlssRenderHeight = primary.Height;
            dlssScene = device.CreateUpscaleTexture(scene.Width, scene.Height, Format.R8G8B8A8Unorm, storage: false);
            dlssUi = device.CreateUpscaleTexture(scene.Width, scene.Height, Format.R8G8B8A8Unorm, storage: false);
            dlssDepth = device.CreateUpscaleTexture(primary.Width, primary.Height, Format.D32Sfloat, storage: false);
            dlssMotion = device.CreateUpscaleTexture(primary.Width, primary.Height, Format.R16G16B16A16Sfloat, storage: false);
        }
        // OUT_OF_DATE/SUBOPTIMAL are handled by the Streamline swapchain dispatch on every frame.
        int presentError = device.TakeStreamlinePresentError();
        if (presentError != 0)
        { Disable("dlss", "Streamline present failed with Vulkan result " + presentError); return; }
        // Query capabilities after valid tags and before the first enable.
        int result = device.TagStreamlineFrame(primary.DepthTextureId, motionTexture,
            scene.Color, ui.Color, dlssDepth, dlssMotion, dlssScene, dlssUi,
            camera, ResetsHistory(frame));
        if (result != 0)
        { Disable("dlss", "Streamline frame tagging failed (" + result + ")"); return; }
        int stateResult = device.GetStreamlineFrameGenerationStateDetails(out var state);
        HeadlessDlssQueryObservation = new DlssQueryObservation(frame.FrameId, stateResult,
            stateResult == 0 ? state : null);
        LastDlssStateResult = stateResult;
        LastDlssState = stateResult == 0 ? state : null;
        if (stateResult != 0)
        {
            if (renderedFrames == 0) { Wait("a valid first tagged DLSS-G capability query"); return; }
            Disable("dlss", stateResult == 39 ? "Streamline reported an out-of-VRAM warning"
                : "Streamline state query/status: " + stateResult + "/" + state.Status);
            return;
        }
        uint maxGenerated = state.MaximumGenerated;
        lastReportedDlssGeneratedLimit = maxGenerated;
        uint presented = state.Presents;
        if (renderedFrames > 0)
        {
            actualDlssPresents += presented;
            device.NoteSdkActualPresents(presented);
        }
        if (maxGenerated == 0) { Wait("a nonzero SDK generated-frame limit"); return; }
        if (scene.Width < state.MinimumSize || scene.Height < state.MinimumSize)
        { ConfigurationWait("output dimensions at least " + state.MinimumSize + " pixels"); return; }
        if (device.PresentationVsyncEnabled && state.VsyncSupport != 1)
        { ConfigurationWait("VSync to be disabled for this DLSS-G runtime"); return; }
        if (state.MinimumSize == 0) { Wait("the SDK minimum output size"); return; }
        if (renderedFrames > 0 && state.Status != 0)
        { Disable("dlss", "Streamline state status: " + state.Status); return; }
        // Tags and the capability query are current; enabling DLSS-G waits for the gate.
        ReadyToGenerate = true;
        if (hold) { Wait(HoldReason); return; }
        if (renderedFrames % 120 == 0)
            notification("VulkanStory: DLSS-G SDK reports " + actualDlssPresents + " frames actually presented over " + renderedFrames + " rendered frames.");
        uint count = Math.Min((uint)Math.Clamp(settings.Multiplier - 1, 1, 5), maxGenerated);
        if (count != configuredDlssCount)
            notification("VulkanStory: DLSS-G requested " + settings.Multiplier + "×, effective " + (count + 1) + "× (SDK maximum " + (maxGenerated + 1) + "×).");
        // The bridge caches unchanged options; calling after tags also resumes a suspended frame.
        result = device.SetStreamlineFrameGeneration(true, count);
        if (result != 0)
        { Disable("dlss", "Streamline DLSS-G options failed (" + result + ")"); return; }
        configuredDlssCount = count;
        renderedFrames++;
        historyResetPending = false;
        PreparedThisFrame = true;
    }

    private unsafe void GenerateXess(IFrameGenerationInputs inputs, FrameGenerationTarget primary, FrameGenerationTarget scene, FrameGenerationTarget ui,
        int motionTexture, in FrameGenerationFrame frame, bool hold)
    {
        if (motionRg == 0 || motionWidth != primary.Width || motionHeight != primary.Height)
        {
            Reset();
            motionRg = device.CreateUpscaleTexture(primary.Width, primary.Height, Format.R16G16Sfloat, storage: false);
            motionWidth = primary.Width; motionHeight = primary.Height;
        }
        // Staging sources would resume the paused presenter; that waits for the gate. The
        // skipped frame resets history and rebases the camera origin on the first prepared one.
        ReadyToGenerate = true;
        if (hold) { Wait(HoldReason); return; }
        bool reset = ResetsHistory(frame);
        var constants = new XessPresentationFrame
        {
            FrameId = (uint)frame.FrameId,
            Reset = reset ? 1u : 0u,
            JitterX = frame.Provider.JitterX, JitterY = -frame.Provider.JitterY,
            MotionScaleX = 1f, MotionScaleY = -1f,
        };
        // Skipped frames do not accumulate camera deltas, so a reset also rebases the origin.
        if (reset) xessCameraX = xessCameraY = xessCameraZ = 0;
        else
        {
            xessCameraX += frame.CameraDeltaX;
            xessCameraY += frame.CameraDeltaY;
            xessCameraZ += frame.CameraDeltaZ;
        }
        inputs.FillXessCamera(ref constants, xessCameraX, xessCameraY, xessCameraZ);
        int result = device.PrepareXessFrame(primary.DepthTextureId, motionTexture,
            motionRg, scene.Color, ui.Color, constants);
        if (result != 0)
        { Disable("xess", "XeSS-FG frame preparation failed (" + result + ")"); return; }
        renderedFrames++;
        historyResetPending = false;
        PreparedThisFrame = true;
    }

    private void Disable(string provider, string reason)
    {
        if (failed.TryAdd(provider, reason)) error("VulkanStory: " + provider + " frame generation unavailable: " + reason);
        Reset();
        SetStatus("unavailable: " + reason);
        device.SetFrameGenerationPresentation("off");
    }
    private void SetStatus(string status)
    {
        preparationStatus = status;
        waitStatusReason = null;
    }
    /// <summary>Whether this frame's provider inputs must discard history: a temporal cut, a fresh provider or a skipped frame.</summary>
    private bool ResetsHistory(in FrameGenerationFrame frame) =>
        frame.Provider.Reset || renderedFrames == 0 || historyResetPending;
    /// <summary>Records a frame that prepares no provider inputs, without changing provider state.</summary>
    private void NoteWait(string reason)
    {
        PreparedThisFrame = false;
        historyResetPending = true;
        if (waitStatusReason != reason)
        {
            preparationStatus = "waiting for " + reason;
            waitStatusReason = reason;
        }
        if (waits.Add(reason)) notification("VulkanStory: frame generation waiting for " + reason);
    }
    /// <summary>Skips this frame like <see cref="Wait" /> for a renderer configuration that waiting alone does not change.</summary>
    private void ConfigurationWait(string reason)
    {
        ConfigurationBlocked = true;
        Wait(reason);
    }
    /// <summary>Skips this frame and pauses the active provider while keeping its inputs and presentation owner.</summary>
    private void Wait(string reason)
    {
        NoteWait(reason);
        SuspendActiveProvider(pause: true);
        if (activeProvider == "fsr3" && fsr3 != null)
        {
            int result = fsr3.Disable(device.Fsr3ProxyContext);
            if (result != 0)
                throw new InvalidOperationException("FidelityFX frame generation suspension/drain failed (" + result + ").");
        }
    }
    /// <summary>Stops DLSS-G or XeSS-FG from consuming current inputs.</summary>
    /// <param name="pause">True for a skipped frame, which keeps the XeSS presenter; false before inputs or the provider change.</param>
    private void SuspendActiveProvider(bool pause)
    {
        if (activeProvider == "dlss") device.SuspendStreamlineFrameGeneration();
        else if (activeProvider == "xess" && pause) device.PauseXessFrameGeneration();
        else if (activeProvider == "xess") device.SuspendXessFrameGeneration();
    }
    /// <summary>Disables the active frame-generation provider and clears input preparation before target/settings transitions.</summary>
    /// <remarks>Checked provider release failures propagate and prevent dependent resource destruction.</remarks>
    internal void Reset()
    {
        RequireOwner();
        // Stop consumers before retiring their input images on the Vulkan timeline.
        SuspendActiveProvider(pause: false);
        device.ResetGeneratedFramePresent();
        PreparedThisFrame = false;
        renderedFrames = 0; actualDlssPresents = configuredDlssCount = 0;
        LastDlssState = null;
        LastDlssStateResult = null;
        if (fsr3 != null)
        {
            int result = fsr3.Disable(device.Fsr3ProxyContext);
            if (result != 0)
                throw new InvalidOperationException("FidelityFX frame generation disable/drain failed (" + result + "); inputs retained.");
            device.RetireUpscalerResource(fsr3);
            fsr3 = null;
        }
        foreach (int texture in new[] { motionRg, fsrScene, fsrUi, fsrDepth, fsrMotion, fsrGenerated, dlssScene, dlssUi, dlssDepth, dlssMotion })
            if (texture > 0) device.DeleteTexture(texture);
        motionRg = fsrScene = fsrUi = fsrDepth = fsrMotion = fsrGenerated = dlssScene = dlssUi = dlssDepth = dlssMotion = 0;
        motionWidth = motionHeight = dlssWidth = dlssHeight = dlssRenderWidth = dlssRenderHeight = 0;
    }
    private void RequireOwner()
    {
        if (disposed) throw new ObjectDisposedException(nameof(FrameGenerationSession));
        if (Environment.CurrentManagedThreadId != ownerThread)
            throw new InvalidOperationException("Frame generation requires the render session owner thread.");
    }
    /// <summary>Resets frame generation and releases its owned preparation state before device teardown.</summary>
    public void Dispose()
    {
        if (disposed) return;
        Reset();
        device.SetFrameGenerationPresentation("off");
        disposed = true;
    }
}

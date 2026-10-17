using VulkanStory.Contracts;

namespace ReScaleFrame.Vulkan;

/// <summary>Owns SR providers, planning, evaluation and checked resource retirement for a Vulkan host.</summary>
internal sealed class UpscalerSession(IUpscalerRuntimeState state, string ngxCachePath, Action<string> log,
    Action requestTargetRebuild) : IDisposable
{
    private readonly Dictionary<string, IUpscalerBackend> providers = new(StringComparer.OrdinalIgnoreCase);
    private readonly Dictionary<string, string> failures = new(StringComparer.OrdinalIgnoreCase);
    private IUpscalerDevice? device;
    private UpscalerPlan allocated, lastEvaluated;
    internal string EvaluationReadiness { get; private set; } = "not evaluated";
    private Exception? lifetimeFailure;
    private void RequireLifetime()
    {
        if (lifetimeFailure != null)
            throw new InvalidOperationException("Upscaler release failed; cleanup is terminal and owners remain retained.", lifetimeFailure);
    }
    private void ReleaseBackend(IUpscalerBackend backend, bool shutdown)
    {
        RequireLifetime();
        try
        {
            if (shutdown) backend.Shutdown();
            else backend.RetireFeature();
        }
        catch (Exception error)
        {
            lifetimeFailure = error;
            throw new InvalidOperationException("ReScaleFrame " + backend.Id + " release failed; dependent cleanup stopped.", error);
        }
    }
    /// <summary>Registers SR backends and contributes their Vulkan device requirements before device initialization.</summary>
    /// <param name="target">Session-owned device receiving requirements and later provider execution.</param>
    /// <remarks>Can run once per registry; the registry borrows the device and owns its provider contexts.</remarks>
    internal void Prepare(IUpscalerDevice target)
    {
        RequireLifetime();
        if (device != null) throw new InvalidOperationException("Upscaler registry is already prepared.");
        device = target;
        DlssUpscaler? dlss = DlssUpscaler.TryPrepare(state, ngxCachePath, log, prepareForSwitching: true);
        if (dlss != null) providers.Add("dlss", new DlssBackend(dlss));
        providers.Add("xess", new XessBackend(log));
        providers.Add("fsr3", new Fsr3Backend(log));
        providers.Add("fsr4", new Fsr4Backend(log));
    }
    internal IEnumerable<IDeviceRequirementContributor> Requirements =>
        providers.Values.Select(provider => provider.Requirements).OfType<IDeviceRequirementContributor>();
    internal bool Active => Selected?.Active == true;
    /// <summary>Initializes registered providers against the created Vulkan device and removes unsupported backends.</summary>
    /// <remarks>Initialization failures become provider refusal reasons. Checked shutdown failures propagate and retain ownership.</remarks>
    internal void BringUp(nint instance, nint physical, nint logical)
    {
        RequireLifetime();
        var target = device ?? throw new InvalidOperationException("Upscaler registry was not prepared before device creation.");
        foreach (var backend in providers.Values.ToArray())
        {
            try
            {
                if (backend.BringUp(target, instance, physical, logical)) continue;
                failures[backend.Id] = backend.Unavailable ?? "provider initialization failed on this Vulkan device";
            }
            catch (Exception error)
            { failures[backend.Id] = error.Message; log("ReScaleFrame " + backend.Id + " bring-up: " + error.Message); }
            ReleaseBackend(backend, shutdown: true);
            providers.Remove(backend.Id);
        }
    }
    private IUpscalerBackend? Selected => providers.GetValueOrDefault(state.EffectiveUpscaler);
    internal string? Unavailable(string provider) => provider == "off" ? null : failures.TryGetValue(provider, out var reason)
        ? reason : state.IsDisabled(provider)
        ? "this provider was disabled for this session" : !providers.TryGetValue(provider, out var backend)
        ? "this provider is unavailable on this Vulkan device" : backend.Active ? null : backend.Unavailable;
    /// <summary>Plans render dimensions and LOD bias for the selected effective SR provider.</summary>
    /// <param name="width">Display width in pixels.</param>
    /// <param name="height">Display height in pixels.</param>
    /// <returns>The selected plan, or null after falling back when no eligible provider can plan.</returns>
    /// <remarks>Clears the prior active plan first; refusal retires the selected feature and requests target rebuild.</remarks>
    internal UpscalerPlan? Plan(int width, int height)
    {
        RequireLifetime();
        allocated = default;
        state.ClearActivePlan();
        var selected = Selected;
        if (selected == null)
        {
            if (state.EffectiveUpscaler != "off") Disable(Unavailable(state.RequestedUpscaler) ?? "the selected provider is unavailable on this device");
            return null;
        }
        if (!selected.TryPlan(width, height, state.Quality, state.LodBiasOffset, out allocated))
        { if (state.EffectiveUpscaler != "off") Disable(selected.Unavailable ?? "the selected upscaler cannot plan a frame on this device"); return null; }
        state.SetActivePlan(allocated.RenderScale, allocated.LodBias);
        return allocated;
    }
    /// <summary>Records a session refusal, retires the selected provider feature and requests ordinary target rebuilding.</summary>
    /// <param name="reason">Concrete provider failure displayed and logged for this session.</param>
    internal void Disable(string reason)
    {
        RequireLifetime();
        var selected = Selected;
        failures[state.RequestedUpscaler] = reason;
        if (state.DisableAtRuntime()) log("ReScaleFrame " + state.RequestedUpscaler + " unavailable: " + reason + "; using the ordinary render path.");
        if (selected != null) ReleaseBackend(selected, shutdown: false);
        allocated = default; state.ClearActivePlan(); requestTargetRebuild();
    }
    /// <summary>Retires current SR features and clears plans before the next settings-driven target allocation.</summary>
    /// <remarks>Provider drain/release failures are terminal for this owner and propagate before dependent cleanup.</remarks>
    internal void ApplySettings()
    {
        RequireLifetime();
        foreach (var backend in providers.Values) ReleaseBackend(backend, shutdown: false);
        allocated = lastEvaluated = default; state.ClearActivePlan();
        requestTargetRebuild();
    }
    /// <summary>Records SR for matching host-prepared scene inputs without taking ownership of the renderer.</summary>
    internal bool Evaluate(in UpscalerFrame frame)
    {
        RequireLifetime();
        IUpscalerBackend? selected = Selected;
        if (selected?.Active != true)
        { EvaluationReadiness = Unavailable(state.RequestedUpscaler) ?? "upscaler not selected"; return false; }
        bool accepted;
        string? error;
        try { accepted = selected.Evaluate(allocated, frame, out error); }
        catch (Exception failure)
        { accepted = false; error = selected.Id + " evaluation threw: " + failure.Message; }
        if (!accepted)
        { EvaluationReadiness = error ?? "provider evaluation failed"; Disable(EvaluationReadiness); return false; }
        if (lastEvaluated != allocated)
        {
            lastEvaluated = allocated;
            log("ReScaleFrame " + selected.Id + " " + allocated.Quality + ": first successful upscale from " +
                allocated.RenderWidth + "x" + allocated.RenderHeight + " to " + allocated.DisplayWidth + "x" + allocated.DisplayHeight + ".");
        }
        EvaluationReadiness = "ready";
        return true;
    }
    /// <summary>Shuts down owned SR providers with checked release and clears the active plan.</summary>
    /// <remarks>The borrowed device is not disposed here; release failure retains the remaining provider owners.</remarks>
    public void Dispose()
    {
        RequireLifetime();
        foreach (var backend in providers.Values.ToArray())
        {
            ReleaseBackend(backend, shutdown: true);
            providers.Remove(backend.Id);
        }
        allocated = lastEvaluated = default; state.ClearActivePlan(); device = null;
    }
}

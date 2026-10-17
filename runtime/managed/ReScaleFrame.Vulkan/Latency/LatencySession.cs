using VulkanStory.Contracts;

namespace ReScaleFrame.Vulkan;

/// <summary>Owns latency settings, frame tokens, pacing authority and marker forwarding.</summary>
internal sealed class LatencySession(ILatencyHost host)
{
    /// <summary>Binds vendor features only after the host has created and registered its device.</summary>
    internal void BringUp(IVulkanProviderContext context, VendorLatencyKind kind)
    {
        if (context.Streamline != null)
        {
            int reflexBind = context.Streamline.BindReflex();
            int fgBind = context.Streamline.BindFrameGeneration();
            int pclResult = context.Streamline.BindPcl();
            _streamlinePclReady = pclResult == 0;
            if (!_streamlinePclReady)
                host.ReportValidation("Streamline PCL unavailable: bind=" + pclResult);
            int reflexResult = reflexBind == 0 ? context.Streamline.SetReflex(DesiredLatencyMode, 0) : -1;
            _streamlineReflexReady = reflexBind == 0 && reflexResult == 0;
            if (!_streamlineReflexReady)
                host.ReportValidation("Streamline Reflex unavailable: bind=" + reflexBind + ", options=" + reflexResult);
            else
            {
                int stateResult = context.Streamline.GetReflexState(out bool available, out bool reports);
                host.ReportValidation("Streamline Reflex state: query=" + stateResult +
                    ", lowLatencyAvailable=" + available + ", latencyReportAvailable=" + reports);
                Console.Error.WriteLine("[VulkanStory] Streamline Reflex: mode=" + DesiredLatencyMode +
                    ", state=" + stateResult + ", available=" + available +
                    ", latencyReports=" + reports);
            }
            _streamlineFrameGenerationReady = fgBind == 0;
            if (!_streamlineFrameGenerationReady)
                host.ReportValidation("Streamline DLSS-G unavailable: bind=" + fgBind);
            if (_streamlineFrameGenerationReady)
            {
                int supportResult = context.Streamline.IsFrameGenerationSupported(context.PhysicalDevice);
                _streamlineFrameGenerationSupported = supportResult == 0;
                if (!_streamlineFrameGenerationSupported)
                    host.ReportValidation("Streamline DLSS-G unsupported on selected Vulkan adapter: " + supportResult);
            }
        }
        _vendorLatency = kind == VendorLatencyKind.Reflex && _streamlineReflexReady
            ? null : VendorLatency.TryCreate(context, kind);
        if (_vendorLatency != null) _vendorLatency.Failure = host.ReportValidation;
    }

    internal FrameTimingRecorder Latency { get; set; } = new();
    /// <summary>Stops competing pacing before the renderer activates a different presentation owner.</summary>
    internal void PresentationProviderChanged()
    {
        if (XessSelected)
        {
            _vendorLatency?.SetMode(0);
            if (_streamlineReflexReady && host.Streamline is { } streamline)
                RequireLatencyProtocol("Streamline", streamline.SetReflex(0, 0), "Reflex off for XeSS pacing handoff");
        }
        _appliedLatencyMode = -1;
    }

    /// <summary>Starts a newly created XeSS presenter in pass-through mode until the next input boundary.</summary>
    internal void XessPresenterCreated()
    {
        _xessProtocolReady = false;
        _xellAppliedFrameCap = -1;
        IXessLatencyPresenter presenter = host.XessPresenter ??
            throw new InvalidOperationException("XeSS presenter was released during latency setup.");
        RequireLatencyProtocol("XeLL", presenter.Runtime.SetLatencyMode(0, false), "initial pass-through options");
        _appliedLatencyMode = -1;
    }

    internal bool _xessProtocolReady;
    internal int _xellAppliedFrameCap = -1;
    internal VendorLatency? _vendorLatency;
    internal bool _streamlineReflexReady;
    internal bool _streamlineFrameGenerationReady;
    internal bool _streamlinePclReady;
    internal bool _streamlineFrameTokenReady;
    internal ulong _streamlineTokenFrameId;
    internal nint _streamlineTokenPointer;
    internal bool _streamlineTokenMismatchLogged;
    internal bool _pclAsyncPresentFailureLogged;
    internal bool _pclPingFailureLogged;
    internal bool _pclMarkerFailureLogged;
    internal bool _xellMarkerFailureLogged;
    internal int _pclPingSuccessCount;
    internal int PclPingSuccessCountForTests => _pclPingSuccessCount;
    internal int _pclMarkerSuccessCount;
    internal int PclMarkerSuccessCountForTests => _pclMarkerSuccessCount;
    internal int _reflexSleepSuccessCount;
    internal int ReflexSleepSuccessCountForTests => _reflexSleepSuccessCount;
    internal bool StreamlineReflexReadyForTests => _streamlineReflexReady;
    internal bool StreamlinePclReadyForTests => _streamlinePclReady;
    internal bool _streamlineFrameGenerationSupported;
    internal int _appliedLatencyMode = -1;
    internal int _vendorFrameCap;
    internal static readonly bool LatencyTraceEnabled =
        Environment.GetEnvironmentVariable("VULKANSTORY_LATENCY_TRACE") == "1";
    internal static void TraceLatency(string message)
    {
        if (LatencyTraceEnabled) Console.Error.WriteLine("[VulkanStory latency] " + message);
    }
    private RendererLatencySelection _latencySelection = new("on", "off");
    /// <summary>Host-selected latency settings snapshot read and written with volatile semantics.</summary>
    public RendererLatencySelection LatencySelection
    {
        get => System.Threading.Volatile.Read(ref _latencySelection);
        set
        {
            ArgumentNullException.ThrowIfNull(value);
            System.Threading.Volatile.Write(ref _latencySelection, value);
        }
    }
    internal int DesiredLatencyMode => LatencySelection.EffectiveMode;
    /// <summary>Whether XeSS-FG is the selected presentation provider; its presenter then owns XeLL pacing instead of the generic vendor latency path.</summary>
    private bool XessSelected => host.XessSelected;
    /// <summary>
    /// The generic vendor latency path (Reflex/Anti-Lag via <see cref="VendorLatency" />), or null while
    /// XeSS-FG is selected and its presenter owns pacing. Only for calls whose XeSS branch does nothing else.
    /// </summary>
    internal VendorLatency? GenericLatency => XessSelected ? null : _vendorLatency;
    /// <summary>Whether the selected active vendor presenter or extension currently owns frame limiting.</summary>
    internal bool VendorLatencyOwnsFrameCap => XessSelected
        ? host.XessPresenter != null
        : _vendorLatency?.OwnsFrameCap == true || _streamlineReflexReady;
    /// <summary>Stores the host FPS limit and forwards it to the active vendor pacing authority.</summary>
    internal void SetVendorLatencyFrameCap(int maxFps)
    {
        _vendorFrameCap = Math.Max(0, maxFps);
        if (host.XessPresenter is { } intel)
        {
            // Until the first pre-input sleep boundary configures XeLL, that boundary applies the cap.
            if (_xessProtocolReady) ApplyXellOptions(intel, "frame-cap options");
            return;
        }
        if (XessSelected) return;
        _vendorLatency?.SetFrameCap(maxFps);
        if (_streamlineReflexReady && host.Streamline is { } streamline)
            RequireLatencyProtocol("Streamline", streamline.SetReflex(DesiredLatencyMode, _vendorFrameCap),
                "Reflex frame-cap options");
    }
    /// <summary>Applies XeLL options when the presenter has not configured them yet or the frame cap changed.</summary>
    /// <remarks>XeSS-FG requires XeLL enabled even when the generic preference is Off, so the generic latency mode does not select these options.</remarks>
    private void ApplyXellOptions(IXessLatencyPresenter intel, string operation)
    {
        if (_xessProtocolReady && _xellAppliedFrameCap == _vendorFrameCap) return;
        // XeLL requires GPU work to be finished before its mode changes.
        intel.WaitForGpuIdle();
        RequireLatencyProtocol("XeLL", intel.Runtime.SetLatencyMode(_vendorFrameCap, true), operation);
        _xessProtocolReady = true;
        _xellAppliedFrameCap = _vendorFrameCap;
    }
    /// <summary>Begins vendor frame identity and performs the selected latency sleep before input collection.</summary>
    internal void SleepVendorLatency(ulong frameId, bool mayGenerate)
    {
        // PCL needs a frame token even when another vendor owns low-latency sleep.
        StreamlineRuntime? streamline = host.Streamline;
        bool streamlineActive = streamline != null &&
            (_streamlinePclReady || _streamlineReflexReady || _streamlineFrameGenerationReady);
        int tokenResult = streamlineActive ? streamline!.BeginFrame(frameId) : -1;
        bool streamlineFrameBegun = tokenResult == 0;
        _streamlineFrameTokenReady = streamlineFrameBegun;
        _streamlineTokenFrameId = streamlineFrameBegun ? frameId : 0;
        _streamlineTokenPointer = streamlineFrameBegun ? streamline!.CurrentFrameToken() : 0;
        if (streamlineActive)
        {
            RequireLatencyProtocol("Streamline", tokenResult, "frame-token creation");
            if (_streamlineTokenPointer == 0)
            {
                _streamlineFrameTokenReady = false;
                throw new InvalidOperationException("Streamline returned an empty token for frame " + frameId + ".");
            }
        }
        if (LatencyTraceEnabled) TraceLatency("frame=" + frameId + " token=" + tokenResult);
        int latencyMode = DesiredLatencyMode;
        if (!XessSelected && _appliedLatencyMode != latencyMode)
        {
            _vendorLatency?.SetMode(latencyMode);
            if (_streamlineReflexReady && streamline != null)
                RequireLatencyProtocol("Streamline", streamline.SetReflex(latencyMode, _vendorFrameCap), "Reflex mode options");
            _appliedLatencyMode = latencyMode;
        }
        // Streamline requires one sleep call per frame even with Reflex Off. Keep
        // it before XeLL or Anti-Lag so their input pacing follows the final sleep.
        if (_streamlineReflexReady && streamlineFrameBegun)
        {
            long sleepStarted = host.BeginReflexWait();
            int result;
            try { result = streamline!.ReflexSleep(); }
            finally { host.EndReflexWait(sleepStarted); }
            RequireLatencyProtocol("Streamline", result, "Reflex sleep");
            _reflexSleepSuccessCount++;
            if (LatencyTraceEnabled) TraceLatency("frame=" + frameId + " reflexSleep=" + result +
                " mode=" + (XessSelected ? 0 : latencyMode));
        }
        if (host.XessPresenter is { } intel)
        {
            ApplyXellOptions(intel, "mode options");
            // XeLL must be enabled before FG and remain enabled for its presenter.
            // A paused presenter resumes generation at the first sleep boundary
            // after a frame prepared complete inputs again.
            if (!host.XessGenerationEnabled && !host.XessGenerationPaused)
                host.SetXessGenerationEnabled(true);
            host.ReserveFramePresentIds(mayGenerate);
            int sleepResult = intel.Runtime.Sleep(frameId);
            RequireLatencyProtocol("XeLL", sleepResult, "sleep");
            if (LatencyTraceEnabled) TraceLatency("frame=" + frameId + " xellSleep=" + sleepResult);
            return;
        }
        if (XessSelected)
        {
            host.ReserveFramePresentIds(mayGenerate);
            return;
        }
        if (host.HasSwapchain) host.ReserveFramePresentIds(mayGenerate);
        _vendorLatency?.Sleep(frameId, host.RealPresentId);
    }
    /// <summary>Throws when a required vendor latency or presentation-protocol call reports failure.</summary>
    /// <param name="vendor">Protocol owner named in the failure, such as Streamline or XeLL.</param>
    internal void RequireLatencyProtocol(string vendor, int result, string operation)
    {
        if (result != 0)
            throw new InvalidOperationException(vendor + " " + operation + " failed (" + result +
                ") at frame " + host.LatencyFrameId + ".");
    }
    /// <summary>Marks the actual input-start boundary for the active AMD latency extension.</summary>
    internal void MarkVendorInputStart(ulong frameId)
    {
        _vendorLatency?.InputStart(frameId);
        if (LatencyTraceEnabled && _vendorLatency?.Kind == VendorLatencyKind.AntiLag)
            TraceLatency("frame=" + frameId + " amdInputStart");
    }
    /// <summary>Records renderer timing and forwards the supported vendor/PCL marker for the current frame.</summary>
    internal void MarkLatency(ulong frameId, LatencyMarker marker)
    {
        Latency.Marker(frameId, marker);
        if (LatencyTraceEnabled) TraceLatency("frame=" + frameId + " boundary=" + marker);
        if (_streamlinePclReady && _streamlineFrameTokenReady &&
            frameId != _streamlineTokenFrameId && !_streamlineTokenMismatchLogged)
        {
            _streamlineTokenMismatchLogged = true;
            host.ReportValidation("Streamline PCL frame-token mismatch: marker frame=" +
                frameId + ", token frame=" + _streamlineTokenFrameId);
        }
        if (_streamlinePclReady && _streamlineFrameTokenReady &&
            frameId == _streamlineTokenFrameId && marker != LatencyMarker.InputSample)
        {
            int result = host.Streamline!.Marker((uint)marker);
            if (result == 0) System.Threading.Interlocked.Increment(ref _pclMarkerSuccessCount);
            if (result != 0 && !_pclMarkerFailureLogged)
            {
                _pclMarkerFailureLogged = true;
                host.ReportValidation("Streamline PCL marker failed: frame=" + frameId +
                    ", marker=" + marker + ", result=" + result);
            }
            if (LatencyTraceEnabled) TraceLatency("frame=" + frameId + " pcl=" + marker + " result=" + result);
        }
        if (marker == LatencyMarker.PresentEnd && frameId == _streamlineTokenFrameId)
            _streamlineFrameTokenReady = false;
        if (host.XessPresenter is { } intel)
        {
            int result = intel.Runtime.Marker(frameId, marker);
            if (result != 0 && !_xellMarkerFailureLogged)
            {
                _xellMarkerFailureLogged = true;
                host.ReportValidation("XeLL marker failed: frame=" + frameId +
                    ", marker=" + marker + ", result=" + result);
            }
            if (LatencyTraceEnabled) TraceLatency("frame=" + frameId + " xell=" + marker + " result=" + result);
            return;
        }
        GenericLatency?.Marker(frameId, marker);
    }
    /// <summary>Registered PCL latency-ping message, or zero when PCL is unavailable.</summary>
    internal uint PclWindowMessage => _streamlinePclReady ? host.Streamline?.PclWindowMessage() ?? 0 : 0;
    /// <summary>Forwards an observed latency-ping boundary to the available Streamline PCL path.</summary>
    internal void MarkPclLatencyPing()
    {
        if (!_streamlinePclReady || !_streamlineFrameTokenReady ||
            host.Streamline is not { } streamline) return;
        int result = streamline.Marker(8); // sl::PCLMarker::ePCLatencyPing
        if (result == 0) _pclPingSuccessCount++;
        if (LatencyTraceEnabled) TraceLatency("frame=" + _streamlineTokenFrameId +
            " pcl=PCLatencyPing result=" + result);
        if (result != 0 && !_pclPingFailureLogged)
        {
            _pclPingFailureLogged = true;
            host.ReportValidation("Streamline PCL ping marker failed: " + result);
        }
    }
    /// <summary>Forwards an asynchronous present marker using the explicitly retained frame token.</summary>
    internal void MarkAsyncPclPresent(ulong frameId, nint token, LatencyMarker marker)
    {
        if (!_streamlinePclReady || token == 0 || host.Streamline is not { } streamline) return;
        int result = streamline.MarkerForToken(token, (uint)marker);
        if (result == 0) System.Threading.Interlocked.Increment(ref _pclMarkerSuccessCount);
        if (LatencyTraceEnabled) TraceLatency("frame=" + frameId + " pclAsync=" + marker +
            " result=" + result);
        if (result != 0 && !_pclAsyncPresentFailureLogged)
        {
            _pclAsyncPresentFailureLogged = true;
            host.ReportValidation("Streamline PCL async present marker failed: frame=" +
                frameId + ", marker=" + marker + ", result=" + result);
        }
    }
}

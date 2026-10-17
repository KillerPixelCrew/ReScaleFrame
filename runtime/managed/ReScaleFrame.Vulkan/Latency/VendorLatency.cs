using System;
using Silk.NET.Vulkan;

using Semaphore = Silk.NET.Vulkan.Semaphore;

namespace ReScaleFrame.Vulkan;

/// <summary>Selected Vulkan vendor-latency extension implementation.</summary>
internal enum VendorLatencyKind { None, Reflex, AntiLag }

/// <summary>Pairs one AMD INPUT mark with the same frame's PRESENT mark.</summary>
internal sealed class AmdAntiLagFrameKey
{
    private ulong _frameId;
    private bool _inputMarked;
    private bool _presentOwed;

    /// <summary>Starts an AMD frame key and clears any input/present pairing from the previous frame.</summary>
    public void BeginFrame(ulong frameId)
    {
        _frameId = frameId;
        _inputMarked = false;
        _presentOwed = false;
    }

    /// <summary>Accepts the first input boundary for the current AMD frame and schedules its paired present marker.</summary>
    public bool InputStart(ulong frameId)
    {
        if (frameId == 0 || frameId != _frameId || _inputMarked) return false;
        _inputMarked = true;
        _presentOwed = true;
        return true;
    }

    /// <summary>Consumes an outstanding AMD present boundary only for the matching current frame.</summary>
    public bool PresentStart(ulong frameId)
    {
        if (frameId != _frameId || !_presentOwed) return false;
        _presentOwed = false;
        return true;
    }

    /// <summary>Cancels an outstanding AMD present marker when no present will occur.</summary>
    public void Cancel() => _presentOwed = false;
}

/// <summary>Enables the supported vendor extension before vkCreateDevice.</summary>
internal sealed unsafe class VendorLatencyRequirements : IDeviceRequirementContributor
{
    private readonly bool _headless;
    private readonly string _choice;

    /// <summary>Reads the vendor-latency selection and disables extension requests for a headless device.</summary>
    public VendorLatencyRequirements(bool headless)
    {
        _headless = headless;
        _choice = Environment.GetEnvironmentVariable("VULKANSTORY_VULKAN_LATENCY")?.Trim().ToLowerInvariant() ?? "auto";
    }

    /// <inheritdoc/>
    public string Name => "vendor latency";
    /// <summary>Vendor implementation selected or loaded for this device.</summary>
    public VendorLatencyKind Kind { get; private set; }

    /// <inheritdoc/>
    public void ContributeInstanceExtensions(InstanceRequirements requirements) { }

    /// <inheritdoc/>
    public void ContributeDeviceRequirements(DeviceRequirements requirements)
    {
        if (_headless || _choice is "off" or "none" or "0") return;
        uint vendor = requirements.Api.GetPhysicalDeviceProperties(requirements.PhysicalDevice).VendorID;
        bool requestNv = _choice is "reflex" or "nv" || (_choice == "auto" && vendor == 0x10de);
        bool requestAmd = _choice is "antilag" or "amd" || (_choice == "auto" && vendor == 0x1002);

        // The context has already requested VK_KHR_present_id and its feature.
        if (requestNv && requirements.IsEnabled("VK_KHR_present_id") &&
            requirements.Request("VK_NV_low_latency2", requestedBy: Name))
        {
            Kind = VendorLatencyKind.Reflex;
            return;
        }

        if (!requestAmd || !requirements.Has("VK_AMD_anti_lag")) return;
        var antiLag = new PhysicalDeviceAntiLagFeaturesAMD
        {
            SType = StructureType.PhysicalDeviceAntiLagFeaturesAmd,
        };
        requirements.QueryFeatures(&antiLag);
        if (!antiLag.AntiLag || !requirements.Request("VK_AMD_anti_lag", requestedBy: Name)) return;
        antiLag.PNext = null;
        requirements.ChainFeature(antiLag);
        Kind = VendorLatencyKind.AntiLag;
    }
}

/// <summary>
/// Drives VK_NV_low_latency2 or VK_AMD_anti_lag on the render thread. The
/// application calls Sleep before sampling input and Marker at the existing
/// frame boundaries. Swapchain creation and retirement are passed in explicitly.
/// </summary>
internal sealed unsafe class VendorLatency : IDisposable
{
    private readonly IVulkanProviderContext _context;
    private readonly NvLowLatency2Functions? _nv;
    private readonly AmdAntiLagFunctions? _amd;
    private readonly Semaphore _sleepSemaphore;
    private SwapchainKHR _swapchain;
    private ulong _sleepValue;
    private ulong _frameId;
    private ulong _presentId;
    private readonly AmdAntiLagFrameKey _amdFrameKey = new();
    private int _maxFps;
    private int _mode = 1;
    private int _appliedMaxFps = -1;
    private bool _reflexReady;
    private bool _disposed;

    private VendorLatency(IVulkanProviderContext context, NvLowLatency2Functions? nv, AmdAntiLagFunctions? amd)
    {
        _context = context;
        _nv = nv;
        _amd = amd;
        if (nv == null) return;
        var type = new SemaphoreTypeCreateInfo
        {
            SType = StructureType.SemaphoreTypeCreateInfo,
            SemaphoreType = SemaphoreType.Timeline,
        };
        var info = new SemaphoreCreateInfo { SType = StructureType.SemaphoreCreateInfo, PNext = &type };
        context.CheckResult(context.Api.CreateSemaphore(context.Device, &info, null, out _sleepSemaphore),
            "vkCreateSemaphore for Reflex sleep");
    }

    /// <summary>Loads extension entry points and creates the selected vendor-latency owner.</summary>
    /// <returns>A latency owner, or null when the selected extension functions are unavailable.</returns>
    public static VendorLatency? TryCreate(IVulkanProviderContext context, VendorLatencyKind kind)
    {
        if (kind == VendorLatencyKind.Reflex)
        {
            NvLowLatency2Functions? nv = NvLowLatency2Functions.Load(context.Api, context.Device);
            return nv == null ? null : new VendorLatency(context, nv, null);
        }
        if (kind == VendorLatencyKind.AntiLag)
        {
            AmdAntiLagFunctions? amd = AmdAntiLagFunctions.Load(context.Api, context.Device);
            return amd == null ? null : new VendorLatency(context, null, amd);
        }
        return null;
    }

    /// <summary>Vendor implementation selected or loaded for this device.</summary>
    public VendorLatencyKind Kind => _nv != null ? VendorLatencyKind.Reflex : VendorLatencyKind.AntiLag;
    /// <summary>Whether the active vendor mode currently owns the renderer frame cap.</summary>
    public bool OwnsFrameCap => !_disposed && _mode != 0 && (_amd != null || _reflexReady);
    /// <summary>Optional renderer-owned sink for a failed native pacing operation.</summary>
    internal Action<string>? Failure { get; set; }

    /// <summary>Clamps latency mode to Off, On or Boost and applies it to the active extension.</summary>
    public void SetMode(int mode)
    {
        int next = Math.Clamp(mode, 0, 2);
        if (_mode == next) return;
        _mode = next;
        if (_nv != null) ApplyReflexMode();
        if (_amd != null)
        {
            _amdFrameKey.Cancel();
            AntiLagUpdate(AntiLagStageAMD.InputAmd, 0, modeOnly: true);
        }
    }

    /// <summary>Stores a nonnegative FPS cap and updates Reflex mode when changed.</summary>
    public void SetFrameCap(int maxFps)
    {
        int cap = Math.Max(0, maxFps);
        if (cap == _maxFps) return;
        _maxFps = cap;
        if (_nv != null) ApplyReflexMode();
    }

    /// <summary>Adds stable Reflex latency-mode creation data to the caller-owned swapchain feature chain.</summary>
    /// <remarks>The returned pointer is owned by this object and remains valid until disposal.</remarks>
    public void* ChainSwapchainCreateInfo(void* next)
    {
        if (_nv == null) return next;
        var info = new SwapchainLatencyCreateInfoNV
        {
            SType = StructureType.SwapchainLatencyCreateInfoNV,
            PNext = next,
            LatencyModeEnable = true,
        };
        // The caller owns the create call, so keep the struct in stable storage
        // across it. This pointer is freed only after all swapchains are retired.
        *_swapchainCreateInfo = info;
        return _swapchainCreateInfo;
    }

    private readonly SwapchainLatencyCreateInfoNV* _swapchainCreateInfo =
        (SwapchainLatencyCreateInfoNV*)System.Runtime.InteropServices.Marshal.AllocHGlobal(sizeof(SwapchainLatencyCreateInfoNV));

    /// <summary>Attaches the newly created swapchain and applies the current Reflex mode.</summary>
    public void OnSwapchainCreated(SwapchainKHR swapchain)
    {
        _swapchain = swapchain;
        ApplyReflexMode();
    }

    /// <summary>Forgets the retired swapchain and cancels pending latency marker pairing.</summary>
    public void OnSwapchainRetired()
    {
        _swapchain = default;
        _reflexReady = false;
        _amdFrameKey.Cancel();
    }

    private void ApplyReflexMode()
    {
        if (_nv == null || _swapchain.Handle == 0 || _disposed) return;
        var mode = new LatencySleepModeInfoNV
        {
            SType = StructureType.LatencySleepModeInfoNV,
            LowLatencyMode = _mode != 0,
            LowLatencyBoost = _mode == 2,
            MinimumIntervalUs = _mode != 0 && _maxFps > 0 ? (uint)Math.Max(1, 1_000_000 / _maxFps) : 0,
        };
        _reflexReady = _nv.SetLatencySleepMode(_context.Device, _swapchain, ref mode) == Result.Success;
    }

    /// <summary>Begins latency frame identity and runs supported Reflex sleep before input polling.</summary>
    /// <remarks>Reflex semaphore waiting is bounded to one second. AMD records identity here and emits its input marker at the separate input boundary.</remarks>
    public void Sleep(ulong frameId, ulong presentId)
    {
        if (_disposed) return;
        _frameId = frameId;
        _presentId = presentId;
        if (_amd != null)
        {
            _amdFrameKey.BeginFrame(frameId);
            // Mode is applied only when the cap changes. The INPUT marker is
            // issued separately at the actual SDL input-poll boundary.
            if (_appliedMaxFps != _maxFps)
            {
                _appliedMaxFps = _maxFps;
                AntiLagUpdate(AntiLagStageAMD.InputAmd, 0, modeOnly: true);
            }
            return;
        }
        if (_nv == null || !_reflexReady || _mode == 0 || _swapchain.Handle == 0) return;
        ulong value = ++_sleepValue;
        var sleep = new LatencySleepInfoNV
        {
            SType = StructureType.LatencySleepInfoNV,
            SignalSemaphore = _sleepSemaphore,
            Value = value,
        };
        Result result = _nv.LatencySleep(_context.Device, _swapchain, ref sleep);
        if (result != Result.Success)
        {
            _reflexReady = false;
            Failure?.Invoke("Native NVIDIA latency sleep failed: " + result);
            return;
        }
        Semaphore semaphore = _sleepSemaphore;
        var wait = new SemaphoreWaitInfo
        {
            SType = StructureType.SemaphoreWaitInfo,
            SemaphoreCount = 1,
            PSemaphores = &semaphore,
            PValues = &value,
        };
        // Bound the wait so a driver failure cannot freeze the client forever.
        Result completion = _context.Api.WaitSemaphores(_context.Device, &wait, 1_000_000_000UL);
        if (completion != Result.Success)
        {
            _reflexReady = false;
            Failure?.Invoke("Native NVIDIA latency sleep did not complete: " + completion);
        }
    }

    /// <summary>Accepts the first input boundary for the current AMD frame and schedules its paired present marker.</summary>
    public void InputStart(ulong frameId)
    {
        if (_disposed || _amd == null || _mode == 0 || !_amdFrameKey.InputStart(frameId)) return;
        AntiLagUpdate(AntiLagStageAMD.InputAmd, frameId, modeOnly: false);
    }

    /// <summary>Emits the supported vendor marker for the current frame or consumes the matching AMD present boundary.</summary>
    public void Marker(ulong frameId, LatencyMarker marker)
    {
        if (_disposed) return;
        if (_amd != null)
        {
            if (marker == LatencyMarker.PresentStart && _amdFrameKey.PresentStart(frameId))
            {
                AntiLagUpdate(AntiLagStageAMD.PresentAmd, frameId, modeOnly: false);
            }
            return;
        }
        if (_nv == null || !_reflexReady || _swapchain.Handle == 0 || frameId != _frameId) return;
        var info = new SetLatencyMarkerInfoNV
        {
            SType = StructureType.SetLatencyMarkerInfoNV,
            PresentID = _presentId,
            Marker = (LatencyMarkerNV)marker,
        };
        _nv.SetLatencyMarker(_context.Device, _swapchain, ref info);
    }

    /// <summary>Cancels an outstanding AMD present marker when no present will occur.</summary>
    public void SkipPresent() => _amdFrameKey.Cancel();

    /// <summary>Attributes a manual generated submit/present to the current real application's present identity.</summary>
    internal void GeneratedMarker(bool rendering, bool start)
    {
        if (_disposed || _nv == null || !_reflexReady || _swapchain.Handle == 0) return;
        var info = new SetLatencyMarkerInfoNV
        {
            SType = StructureType.SetLatencyMarkerInfoNV,
            PresentID = _presentId,
            Marker = (LatencyMarkerNV)((rendering ? 8 : 10) + (start ? 0 : 1)),
        };
        _nv.SetLatencyMarker(_context.Device, _swapchain, ref info);
    }

    private void AntiLagUpdate(AntiLagStageAMD stage, ulong frameId, bool modeOnly)
    {
        var presentation = new AntiLagPresentationInfoAMD
        {
            SType = StructureType.AntiLagPresentationInfoAmd,
            Stage = stage,
            FrameIndex = frameId,
        };
        var data = new AntiLagDataAMD
        {
            SType = StructureType.AntiLagDataAmd,
            Mode = _mode == 0 ? AntiLagModeAMD.OffAmd : AntiLagModeAMD.OnAmd,
            MaxFps = _mode == 0 ? 0u : (uint)_maxFps,
            PPresentationInfo = modeOnly ? null : &presentation,
        };
        _amd!.AntiLagUpdate(_context.Device, ref data);
    }

    /// <inheritdoc/>
    public void Dispose()
    {
        if (_disposed) return;
        if (_amd != null)
        {
            var data = new AntiLagDataAMD { SType = StructureType.AntiLagDataAmd, Mode = AntiLagModeAMD.OffAmd };
            _amd.AntiLagUpdate(_context.Device, ref data);
        }
        _disposed = true;
        if (_sleepSemaphore.Handle != 0)
            _context.Api.DestroySemaphore(_context.Device, _sleepSemaphore, null);
        System.Runtime.InteropServices.Marshal.FreeHGlobal((nint)_swapchainCreateInfo);
    }
}

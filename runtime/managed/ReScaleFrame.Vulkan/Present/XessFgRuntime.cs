using System;
using System.Collections.Generic;
using System.IO;
using System.Runtime.InteropServices;
using Silk.NET.Vulkan;

namespace ReScaleFrame.Vulkan;

/// <summary>
/// Owns the matching DX12 device, XeLL context, and Intel XeSS-FG context.
/// The DXGI proxy is initialized only after Vulkan relinquishes the window.
/// </summary>
internal sealed unsafe class XessFgRuntime : IDisposable, IDx12SharedRuntime
{
    // Failed bring-up has no presenter owner. Keep its native context/module
    // reachable and pinned instead of unloading code after an unsuccessful release.
    private static readonly System.Collections.Generic.List<XessFgRuntime> FailedCreates = new();
    private nint _module;
    private nint _context;
    private readonly object _sleptFramesGate = new();
    private readonly HashSet<ulong> _sleptFrames = new();
    private readonly delegate* unmanaged[Cdecl]<nint, int> _destroy;
    private readonly delegate* unmanaged[Cdecl]<nint, int> _prepareDestroy;
    private Exception? _releaseFailure;
    private readonly delegate* unmanaged[Cdecl]<nint> _lastCreateStage;
    private readonly delegate* unmanaged[Cdecl]<nint, nint, uint, uint, uint, int> _start;
    private readonly delegate* unmanaged[Cdecl]<nint, uint, int> _setEnabled;
    private readonly delegate* unmanaged[Cdecl]<nint, uint, uint*, uint*, int> _setGeneratedFrames;
    private readonly delegate* unmanaged[Cdecl]<nint, uint, uint, int> _setLatencyMode;
    private readonly delegate* unmanaged[Cdecl]<nint, uint, int> _sleep;
    private readonly delegate* unmanaged[Cdecl]<nint, uint, int, int> _marker;
    private readonly delegate* unmanaged[Cdecl]<nint, uint, uint, uint, nint*, nint*, int> _createSharedImage;
    private readonly delegate* unmanaged[Cdecl]<nint, void> _releaseImage;
    private readonly delegate* unmanaged[Cdecl]<nint, nint*, int> _createSharedFence;
    private readonly delegate* unmanaged[Cdecl]<nint, XessPresentationFrame*, uint*, int*, uint*, int> _present;
    private readonly delegate* unmanaged[Cdecl]<nint, int> _waitIdle;

    private XessFgRuntime(nint module, nint context)
    {
        _module = module;
        _context = context;
        _destroy = (delegate* unmanaged[Cdecl]<nint, int>)Export("VulkanStoryXessFgDestroyChecked");
        _prepareDestroy = (delegate* unmanaged[Cdecl]<nint, int>)Export("VulkanStoryXessFgPrepareDestroy");
        _lastCreateStage = NativeLibrary.TryGetExport(module, "VulkanStoryXessFgLastCreateStage", out nint stage)
            ? (delegate* unmanaged[Cdecl]<nint>)stage : null;
        _start = (delegate* unmanaged[Cdecl]<nint, nint, uint, uint, uint, int>)Export("VulkanStoryXessFgStart");
        _setEnabled = (delegate* unmanaged[Cdecl]<nint, uint, int>)Export("VulkanStoryXessFgSetEnabled");
        _setGeneratedFrames = (delegate* unmanaged[Cdecl]<nint, uint, uint*, uint*, int>)Export("VulkanStoryXessFgSetGeneratedFrames");
        _setLatencyMode = (delegate* unmanaged[Cdecl]<nint, uint, uint, int>)Export("VulkanStoryXessFgSetLatencyMode");
        _sleep = (delegate* unmanaged[Cdecl]<nint, uint, int>)Export("VulkanStoryXessFgSleep");
        _marker = (delegate* unmanaged[Cdecl]<nint, uint, int, int>)Export("VulkanStoryXessFgMarker");
        _createSharedImage = (delegate* unmanaged[Cdecl]<nint, uint, uint, uint, nint*, nint*, int>)Export("VulkanStoryXessFgCreateSharedImage");
        _releaseImage = (delegate* unmanaged[Cdecl]<nint, void>)Export("VulkanStoryXessFgReleaseImage");
        _createSharedFence = (delegate* unmanaged[Cdecl]<nint, nint*, int>)Export("VulkanStoryXessFgCreateSharedFence");
        _present = (delegate* unmanaged[Cdecl]<nint, XessPresentationFrame*, uint*, int*, uint*, int>)Export("VulkanStoryXessFgPresent");
        _waitIdle = (delegate* unmanaged[Cdecl]<nint, int>)Export("VulkanStoryXessFgWaitIdle");
    }

    private nint Export(string symbol) => NativeLibrary.GetExport(_module, symbol);

    /// <summary>Loads the private XeSS-FG bridge and creates its DX12 context on the Vulkan-selected adapter.</summary>
    /// <returns>Whether context creation succeeded; reason includes the recorded native creation stage on failure.</returns>
    public static bool TryCreate(IVulkanProviderContext vulkan, out XessFgRuntime? runtime,
        out string reason)
    {
        runtime = null;
        if (!OperatingSystem.IsWindows()) { reason = "XeSS-FG requires Windows DX12"; return false; }
        string path = Path.Combine(NativeRuntimePaths.DirectoryContaining("VulkanStoryXessFg.dll"),
            "VulkanStoryXessFg.dll");
        if (!File.Exists(path)) { reason = "XeSS-FG DX12 bridge is not installed"; return false; }
        nint module = 0;
        nint context = 0;
        XessFgRuntime? instance = null;
        try
        {
            module = NativeRuntimePaths.LoadLibrary(path);
            // Bind required exports before creating a native owner. Old bridges
            // cannot silently fall back to unchecked destruction.
            instance = new XessFgRuntime(module, 0);
            var create = (delegate* unmanaged[Cdecl]<PhysicalDevice, nint*, int>)
                NativeLibrary.GetExport(module, "VulkanStoryXessFgCreateFromVulkan");
            int result = create(vulkan.PhysicalDevice, &context);
            instance._context = context;
            if (result != 0 || context == 0)
            {
                string stage = instance._lastCreateStage != null
                    ? Marshal.PtrToStringAnsi(instance._lastCreateStage()) ?? "unknown stage" : "adapter/DX12/XeLL";
                reason = "XeSS-FG initialization failed at " + stage + " (" + result + ")";
                if (stage == "xellD3D12CreateContext" && result == -2)
                    reason += "; XeLL reports an unsupported driver; frame generation remains disabled.";
                instance.Dispose();
                return false;
            }
            runtime = instance;
            reason = "ready";
            return true;
        }
        catch (Exception error)
        {
            if (instance != null)
            {
                try { instance.Dispose(); }
                catch (Exception cleanup)
                {
                    lock (FailedCreates) FailedCreates.Add(instance);
                    runtime = instance;
                    reason = "XeSS-FG initialization/release failed; native owner retained: " + cleanup.Message;
                    return false;
                }
            }
            else if (module != 0) NativeLibrary.Free(module);
            reason = "XeSS-FG bridge unavailable: " + error.Message;
            return false;
        }
    }

    /// <summary>Starts the native presenter for a borrowed Win32 window and display extent.</summary>
    public int Start(nint window, uint width, uint height, bool vsync) =>
        _context != 0 ? _start(_context, window, width, height, vsync ? 1u : 0u) : -1;
    /// <summary>Applies native frame-generation enablement and returns the bridge result.</summary>
    public int SetEnabled(bool enabled) => _context != 0 ? _setEnabled(_context, enabled ? 1u : 0u) : -1;
    /// <summary>Negotiates the requested interpolation count with the runtime.</summary>
    /// <param name="requested">Requested generated frames per rendered frame.</param>
    /// <param name="effective">Count actually configured by the bridge.</param>
    /// <param name="maximum">Runtime-reported generated-frame limit.</param>
    /// <returns>Native bridge result code; zero indicates success.</returns>
    public int SetGeneratedFrames(uint requested, out uint effective, out uint maximum)
    {
        effective = maximum = 0;
        fixed (uint* effectivePtr = &effective)
        fixed (uint* maximumPtr = &maximum)
            return _context != 0 ? _setGeneratedFrames(_context, requested,
                effectivePtr, maximumPtr) : -1;
    }
    /// <summary>Applies XeLL enablement and a nonnegative frame cap through the presenter bridge.</summary>
    public int SetLatencyMode(int maxFps, bool enabled) => _context != 0 ?
        _setLatencyMode(_context, maxFps > 0 ? (uint)Math.Max(1, 1_000_000 / maxFps) : 0,
            enabled ? 1u : 0u) : -1;
    /// <summary>Runs native XeLL sleep using the low 32 bits of the renderer frame ID.</summary>
    /// <remarks>Registers a successful sleep until its PresentEnd marker, including while later frames record on the render thread.</remarks>
    public int Sleep(ulong frameId)
    {
        int result = _context != 0 ? _sleep(_context, (uint)frameId) : -1;
        if (result == 0)
            lock (_sleptFramesGate) _sleptFrames.Add(frameId);
        return result;
    }
    /// <summary>Emits a native XeLL latency marker for the supplied frame ID.</summary>
    /// <remarks>Suppresses the initial pass-through frame without sleep; asynchronous presentation retains eligibility for earlier slept frames.</remarks>
    public int Marker(ulong frameId, LatencyMarker marker)
    {
        if (_context == 0) return -1;
        lock (_sleptFramesGate)
            if (!_sleptFrames.Contains(frameId)) return 0;
        int result = _marker(_context, (uint)frameId, (int)marker);
        if (marker == LatencyMarker.PresentEnd)
            lock (_sleptFramesGate) _sleptFrames.Remove(frameId);
        return result;
    }
    /// <inheritdoc/>
    public int CreateSharedImage(uint width, uint height, Format format, bool writable,
        out nint sharedHandle, out nint resource)
    {
        if (writable) { sharedHandle = resource = 0; return -1; }
        nint handle = 0;
        nint created = 0;
        int code = _context != 0 ?
            _createSharedImage(_context, width, height, (uint)format, &handle, &created) : -1;
        sharedHandle = handle;
        resource = created;
        return code;
    }
    /// <inheritdoc/>
    public void ReleaseImage(nint resource)
    {
        if (resource != 0) _releaseImage(resource);
    }
    /// <inheritdoc/>
    public int CreateSharedFence(out nint sharedHandle)
    {
        nint handle = 0;
        int code = _context != 0 ? _createSharedFence(_context, &handle) : -1;
        sharedHandle = handle;
        return code;
    }
    /// <summary>Waits for the native DX12 queue to finish its tracked work.</summary>
    /// <returns>Zero on successful completion; a nonzero bridge code on failure or an absent context.</returns>
    public int WaitIdle() => _context != 0 ? _waitIdle(_context) : -1;
    /// <summary>Presents a prepared shared-image frame and reports native presentation/interpolation outcomes.</summary>
    /// <returns>The native presentation result code; output counters describe the bridge-reported outcome.</returns>
    public int Present(in XessPresentationFrame frame, out uint framesPresented,
        out int frameGenResult, out bool frameGenEnabled)
    {
        XessPresentationFrame copy = frame;
        uint presented = 0;
        int result = 0;
        uint enabled = 0;
        int code = _context != 0 ? _present(_context, &copy, &presented, &result, &enabled) : -1;
        framesPresented = presented;
        frameGenResult = result;
        frameGenEnabled = enabled != 0;
        return code;
    }

    private bool releasePrepared;
    /// <summary>Prepares checked native-context release before dependent imported resources are destroyed.</summary>
    /// <remarks>Release failure is retained and subsequent release attempts throw without relinquishing ownership.</remarks>
    /// <exception cref="InvalidOperationException">The native bridge cannot safely prepare release, or an earlier release failed.</exception>
    internal void PrepareRelease()
    {
        if (_releaseFailure != null)
            throw new InvalidOperationException("XeSS-FG runtime release failed; remaining owners are retained.", _releaseFailure);
        if (_context == 0 || releasePrepared) return;
        try
        {
            int result = _prepareDestroy(_context);
            if (result != 0) throw new InvalidOperationException("XeSS-FG native release preparation failed (" + result + ").");
            releasePrepared = true;
        }
        catch (Exception failure) { _releaseFailure = failure; throw; }
    }
    /// <inheritdoc/>
    public void Dispose()
    {
        PrepareRelease();
        if (_context != 0)
        {
            int result = _destroy(_context);
            if (result != 0)
            {
                _releaseFailure = new InvalidOperationException("XeSS-FG native destruction failed (" + result + ").");
                throw _releaseFailure;
            }
            _context = 0;
        }
        nint module = _module;
        _module = 0;
        if (module != 0) NativeLibrary.Free(module);
    }
}

// SPDX-License-Identifier: GPL-3.0-only
using System;
using System.Globalization;
using System.Threading;

namespace ReScaleFrame.Vulkan;

/// <summary>Excludes another native or managed orchestrator from pacing this process.</summary>
internal sealed class LatencyOwnerLease : IDisposable
{
    private readonly EventWaitHandle? _event;
    private LatencyOwnerLease(EventWaitHandle? handle) => _event = handle;

    /// <summary>The first active orchestrator owns the Windows process pacing lease.</summary>
    public static LatencyOwnerLease? TryAcquire()
    {
        // The native orchestrator is Windows-only, so there is no cross-runtime collision on Unix.
        if (!OperatingSystem.IsWindows()) return new LatencyOwnerLease(null);
        string name = "Local\\ReScaleFrame.LatencyOwner." + Environment.ProcessId.ToString(CultureInfo.InvariantCulture);
        var handle = new EventWaitHandle(false, EventResetMode.ManualReset, name, out bool created);
        if (created) return new LatencyOwnerLease(handle);
        handle.Dispose();
        return null;
    }

    /// <inheritdoc/>
    public void Dispose() => _event?.Dispose();
}

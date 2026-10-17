using System;
using System.Diagnostics;
using System.Threading;

namespace ReScaleFrame.Vulkan;

/// <summary>Spaces the real present halfway between successive generated presents.</summary>
internal sealed class GeneratedFramePacer
{
    private readonly Func<long> now;
    private readonly Action<int> sleep;
    private readonly bool precise;
    private long lastGenerated;
    private long realTarget;
    private double renderIntervalTicks = Stopwatch.Frequency / 60.0;

    /// <summary>Creates a CPU present pacer with optional clock and sleep functions.</summary>
    /// <param name="now">Timestamp source in Stopwatch ticks; defaults to Stopwatch.GetTimestamp.</param>
    /// <param name="sleep">Millisecond sleep callback; defaults to Thread.Sleep.</param>
    /// <remarks>Without injected functions the wait uses <see cref="PreciseSleep" />; Thread.Sleep alone rounds to the ~15.6 ms system timer period.</remarks>
    internal GeneratedFramePacer(Func<long>? now = null, Action<int>? sleep = null)
    {
        precise = now == null && sleep == null;
        this.now = now ?? Stopwatch.GetTimestamp;
        this.sleep = sleep ?? Thread.Sleep;
    }

    /// <summary>Samples generated-present cadence and schedules the real present at half the smoothed render interval.</summary>
    internal void NoteGeneratedPresent()
    {
        long timestamp = now();
        if (lastGenerated != 0)
        {
            long elapsed = timestamp - lastGenerated;
            if (elapsed > 0 && elapsed < Stopwatch.Frequency / 10)
                renderIntervalTicks = Math.Clamp(renderIntervalTicks * 0.875 + elapsed * 0.125,
                    Stopwatch.Frequency / 1000.0, Stopwatch.Frequency / 10.0);
        }
        lastGenerated = timestamp;
        realTarget = timestamp + (long)(renderIntervalTicks * 0.5);
    }

    /// <summary>Next real-present target in Stopwatch ticks, or zero when no wait is scheduled.</summary>
    internal long PendingRealTargetTicks => realTarget;

    /// <summary>Consumes the pending real-present target and blocks until its clock deadline.</summary>
    /// <remarks>Uses a high-resolution coarse wait (or the injected millisecond sleep) and spinning for the final interval. Call on the presentation owner thread.</remarks>
    internal void WaitForRealPresent()
    {
        long target = realTarget;
        realTarget = 0;
        if (target == 0) return;
        if (precise)
        {
            PreciseSleep.Until(target);
            return;
        }
        while (true)
        {
            long remaining = target - now();
            if (remaining <= 0) return;
            int ms = (int)(remaining * 1000 / Stopwatch.Frequency);
            if (ms > 1) sleep(ms - 1);
            else Thread.SpinWait(32);
        }
    }

    /// <summary>Clears pending timing and restores the initial 60 Hz interval estimate.</summary>
    internal void Reset()
    {
        lastGenerated = 0;
        realTarget = 0;
        renderIntervalTicks = Stopwatch.Frequency / 60.0;
    }
}

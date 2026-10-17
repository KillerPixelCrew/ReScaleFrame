using System;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Threading;

namespace ReScaleFrame.Vulkan;

/// <summary>
/// Waits until a <see cref="Stopwatch" /> deadline with sub-millisecond accuracy.
/// <c>Thread.Sleep(int)</c> rounds to the system timer period, which is about
/// 15.6 ms unless a process raised the timer resolution. This sleeps coarsely
/// through a high-resolution waitable timer on Windows (Thread.Sleep with a
/// margin elsewhere or when that timer is unavailable) and spins for the final
/// interval.
/// </summary>
internal static unsafe class PreciseSleep
{
    private const uint CreateWaitableTimerHighResolution = 0x00000002;
    private const uint TimerAllAccess = 0x001F0003;
    private const uint Infinite = 0xFFFFFFFF;
    private const uint WaitObject0 = 0;

    /// <summary>The final interval that is always spun instead of slept.</summary>
    private static readonly long SpinTicks = Stopwatch.Frequency / 1000;

    /// <summary>Extra headroom when the coarse wait is a plain Thread.Sleep.</summary>
    private static readonly long SleepMarginTicks = Stopwatch.Frequency * 2 / 1000;

    // One timer per sleeping thread: a waitable timer has a single due time, so
    // threads must not re-arm a shared one. The handle lives as long as its thread.
    [ThreadStatic] private static nint t_timer;
    [ThreadStatic] private static bool t_timerUnavailable;

    /// <summary>Blocks the calling thread until <paramref name="targetStopwatchTimestamp" />; returns at once when it has passed.</summary>
    /// <param name="targetStopwatchTimestamp">Deadline in <see cref="Stopwatch.GetTimestamp" /> ticks.</param>
    internal static void Until(long targetStopwatchTimestamp)
    {
        long remaining = targetStopwatchTimestamp - Stopwatch.GetTimestamp();
        if (remaining <= 0) return;
        if (remaining > SpinTicks) SleepCoarse(remaining - SpinTicks);
        while (Stopwatch.GetTimestamp() < targetStopwatchTimestamp) Thread.SpinWait(32);
    }

    /// <summary>Blocks the calling thread for <paramref name="milliseconds" />; nonpositive values return at once.</summary>
    internal static void For(double milliseconds)
    {
        if (!(milliseconds > 0)) return;
        Until(Stopwatch.GetTimestamp() + (long)(milliseconds * Stopwatch.Frequency / 1000.0));
    }

    private static void SleepCoarse(long ticks)
    {
        if (OperatingSystem.IsWindows() && TryWaitTimer(ticks)) return;
        long sleepTicks = ticks - SleepMarginTicks;
        if (sleepTicks <= 0) return;
        int milliseconds = (int)Math.Min(int.MaxValue, sleepTicks * 1000 / Stopwatch.Frequency);
        if (milliseconds > 0) Thread.Sleep(milliseconds);
    }

    private static bool TryWaitTimer(long ticks)
    {
        if (t_timerUnavailable) return false;
        nint timer = t_timer;
        if (timer == 0)
        {
            // CREATE_WAITABLE_TIMER_HIGH_RESOLUTION needs Windows 10 1803 or later.
            timer = CreateWaitableTimerEx(0, 0, CreateWaitableTimerHighResolution, TimerAllAccess);
            if (timer == 0)
            {
                t_timerUnavailable = true;
                return false;
            }
            t_timer = timer;
        }
        // Negative due times are relative, in 100 ns units.
        long due = -(long)(ticks * (10_000_000.0 / Stopwatch.Frequency));
        if (due >= 0) return true;
        if (!SetWaitableTimer(timer, &due, 0, 0, 0, false)) return false;
        return WaitForSingleObject(timer, Infinite) == WaitObject0;
    }

    [DllImport("kernel32.dll", EntryPoint = "CreateWaitableTimerExW")]
    private static extern nint CreateWaitableTimerEx(nint attributes, nint name, uint flags, uint desiredAccess);

    [DllImport("kernel32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool SetWaitableTimer(nint timer, long* dueTime, int period,
        nint completionRoutine, nint completionArgument, [MarshalAs(UnmanagedType.Bool)] bool resume);

    [DllImport("kernel32.dll")]
    private static extern uint WaitForSingleObject(nint handle, uint milliseconds);
}

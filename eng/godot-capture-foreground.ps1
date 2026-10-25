# Process-scoped foreground control for an explicitly requested GPU fixture.
if (!('RsfGodotCaptureFocus' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class RsfGodotCaptureFocus {
    public sealed class Observation {
        public uint before_pid, after_pid;
        public long before_window, after_window, target_window;
        public int owned_windows, visible_windows;
        public bool attached_target, attached_foreground, activated;
        public int target_attach_error, foreground_attach_error;
        public uint cloaked;
        public bool? on_current_desktop;
        public string target_class, target_desktop, foreground_desktop;
        public bool attempted, owned;
    }
    [StructLayout(LayoutKind.Sequential)] struct Message {
        public IntPtr window; public uint message; public UIntPtr wParam;
        public IntPtr lParam; public uint time; public int x, y; public uint privateValue;
    }
    delegate bool WindowCallback(IntPtr window, IntPtr user);
    [DllImport("user32.dll")] static extern bool EnumWindows(WindowCallback callback, IntPtr user);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr window);
    [DllImport("user32.dll")] static extern IntPtr GetWindow(IntPtr window, uint command);
    [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr window, out uint process);
    [DllImport("kernel32.dll")] static extern uint GetCurrentThreadId();
    [DllImport("user32.dll", SetLastError=true)] static extern bool AttachThreadInput(uint from, uint to, bool attach);
    [DllImport("user32.dll")] static extern bool BringWindowToTop(IntPtr window);
    [DllImport("user32.dll")] static extern bool SetForegroundWindow(IntPtr window);
    [DllImport("user32.dll", EntryPoint="PeekMessageW")] static extern bool PeekMessage(out Message message, IntPtr window, uint first, uint last, uint remove);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassNameW(IntPtr window, System.Text.StringBuilder name, int size);
    [DllImport("dwmapi.dll")] static extern int DwmGetWindowAttribute(IntPtr window, uint attribute, out uint value, uint bytes);
    [ComImport, Guid("a5cd92ff-29be-454c-8d04-d82879fb3f1b"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    interface DesktopManager {
        [PreserveSig] int IsWindowOnCurrentVirtualDesktop(IntPtr window, [MarshalAs(UnmanagedType.Bool)] out bool current);
        [PreserveSig] int GetWindowDesktopId(IntPtr window, out Guid desktop);
    }
    static uint Process(IntPtr window) { uint value; GetWindowThreadProcessId(window, out value); return value; }
    public static Observation Sample(uint process, bool activate) {
        var result = new Observation();
        IntPtr before = GetForegroundWindow();
        result.before_window = before.ToInt64(); result.before_pid = Process(before);
        IntPtr target = IntPtr.Zero;
        EnumWindows((window, user) => {
            if (Process(window) == process) {
                result.owned_windows++;
                if (IsWindowVisible(window)) {
                    result.visible_windows++;
                    if (target == IntPtr.Zero || GetWindow(window, 4) == IntPtr.Zero) target = window;
                }
            }
            return true;
        }, IntPtr.Zero);
        result.target_window = target.ToInt64();
        if (target != IntPtr.Zero) {
            var name = new System.Text.StringBuilder(128); GetClassNameW(target, name, 128); result.target_class = name.ToString();
            uint cloaked; if (DwmGetWindowAttribute(target, 14, out cloaked, 4) == 0) result.cloaked = cloaked;
            object manager = null;
            try {
                manager = Activator.CreateInstance(Type.GetTypeFromCLSID(new Guid("aa509086-5ca9-4c25-8f95-589d3c07b48a")));
                var desktop = (DesktopManager)manager; bool current; Guid id;
                if (desktop.IsWindowOnCurrentVirtualDesktop(target, out current) == 0) result.on_current_desktop = current;
                if (desktop.GetWindowDesktopId(target, out id) == 0) result.target_desktop = id.ToString();
                if (desktop.GetWindowDesktopId(before, out id) == 0) result.foreground_desktop = id.ToString();
            } catch (COMException) {} finally { if (manager != null) Marshal.ReleaseComObject(manager); }
        }
        if (activate && result.before_pid != process && target != IntPtr.Zero && Process(target) == process) {
            result.attempted = true;
            // Establish this thread's input queue. Attach only for activation,
            // restoring every temporary relationship before returning.
            Message message; PeekMessage(out message, IntPtr.Zero, 0, 0, 0);
            uint current = GetCurrentThreadId(), ignored;
            uint foreground = GetWindowThreadProcessId(before, out ignored);
            uint owner = GetWindowThreadProcessId(target, out ignored);
            bool a = owner != 0 && owner != current && AttachThreadInput(current, owner, true);
            result.attached_target = a; if (!a) result.target_attach_error = Marshal.GetLastWin32Error();
            bool b = foreground != 0 && foreground != current && foreground != owner && AttachThreadInput(current, foreground, true);
            result.attached_foreground = b; if (!b) result.foreground_attach_error = Marshal.GetLastWin32Error();
            try { BringWindowToTop(target); result.activated = SetForegroundWindow(target); }
            finally {
                if (b) AttachThreadInput(current, foreground, false);
                if (a) AttachThreadInput(current, owner, false);
            }
        }
        IntPtr after = GetForegroundWindow();
        result.after_window = after.ToInt64(); result.after_pid = Process(after);
        result.owned = result.after_pid == process;
        return result;
    }
}
'@
}

function Update-RsfCaptureForeground([Diagnostics.Process]$Process, [hashtable]$Receipt, [Diagnostics.Stopwatch]$Timer, [string]$Phase) {
    if ($Process.HasExited) { return $false }
    $taskObservation = [RsfGodotCaptureFocus]::Sample([uint32]$Process.Id, $true)
    if ($taskObservation.before_pid -ne $Process.Id) {
        if ($Phase -eq 'runtime') { $Receipt.foreground_lost_samples++ } else { $Receipt.startup_lost_samples++ }
    }
    if ($taskObservation.attempted) {
        $Receipt.reassert_attempts++
        if ($taskObservation.owned) { $Receipt.reassert_successes++ }
    }
    $Receipt.observations.Add(@{elapsed_ms=$Timer.ElapsedMilliseconds; phase=$Phase;
        before_pid=$taskObservation.before_pid; before_window=$taskObservation.before_window;
        after_pid=$taskObservation.after_pid; after_window=$taskObservation.after_window;
        target_window=$taskObservation.target_window; attempted=$taskObservation.attempted; owned=$taskObservation.owned})
    $Receipt.observations[$Receipt.observations.Count - 1].owned_windows = $taskObservation.owned_windows
    $Receipt.observations[$Receipt.observations.Count - 1].visible_windows = $taskObservation.visible_windows
    $Receipt.observations[$Receipt.observations.Count - 1].target_class = $taskObservation.target_class
    $Receipt.observations[$Receipt.observations.Count - 1].cloaked = $taskObservation.cloaked
    $Receipt.observations[$Receipt.observations.Count - 1].on_current_desktop = $taskObservation.on_current_desktop
    $Receipt.observations[$Receipt.observations.Count - 1].target_desktop = $taskObservation.target_desktop
    $Receipt.observations[$Receipt.observations.Count - 1].foreground_desktop = $taskObservation.foreground_desktop
    $Receipt.observations[$Receipt.observations.Count - 1].attached_target = $taskObservation.attached_target
    $Receipt.observations[$Receipt.observations.Count - 1].attached_foreground = $taskObservation.attached_foreground
    $Receipt.observations[$Receipt.observations.Count - 1].target_attach_error = $taskObservation.target_attach_error
    $Receipt.observations[$Receipt.observations.Count - 1].foreground_attach_error = $taskObservation.foreground_attach_error
    $Receipt.observations[$Receipt.observations.Count - 1].activated = $taskObservation.activated
    return $taskObservation.owned
}

# These helpers perform no waits. The caller owns the startup/overall deadline
# and calls Update once per bounded poll while its owned process remains live.
function New-RsfCaptureForegroundReceipt([Diagnostics.Process]$Process) {
    return @{requested=$true; process_id=$Process.Id; foreground_lost_samples=0;
        startup_lost_samples=0; reassert_attempts=0; reassert_successes=0;
        observations=[Collections.Generic.List[object]]::new()}
}

function Save-RsfCaptureForegroundReceipt([hashtable]$Receipt, [string]$Path) {
    $taskFinalOwner = [RsfGodotCaptureFocus]::Sample([uint32]$Receipt.process_id, $false)
    $Receipt.final_owner_after_capture = @{process_id=$taskFinalOwner.after_pid; window=$taskFinalOwner.after_window}
    $Receipt | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $Path -Encoding utf8
}

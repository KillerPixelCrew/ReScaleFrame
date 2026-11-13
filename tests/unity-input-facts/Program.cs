// SPDX-License-Identifier: GPL-3.0-only
using System;
using System.Collections.Generic;
using System.Threading;
using System.Globalization;
using ReScaleFrame.Engine;
using ReScaleFrame.Vulkan;

internal static class Program
{
    public struct Event { public string type { get; set; } public int deviceId { get; set; } }
    public class Keyboard { }
    public class Mouse { }
    public class Gamepad { }
    public class CustomController : Gamepad { }
    public sealed class Stream : IObservable<Event>
    {
        private IObserver<Event> current;
        private sealed class Lease : IDisposable { private readonly Stream owner; public Lease(Stream value) => owner=value; public void Dispose() => owner.current=null; }
        public IDisposable Subscribe(IObserver<Event> observer) { current=observer;return new Lease(this); }
        public void Send(string type,int device) => current?.OnNext(new Event {type=type,deviceId=device});
    }
    public static class InputSystem
    {
        public static Stream onEvent { get; } = new Stream();
        public static object GetDeviceById(int id) => id switch {1 => new Keyboard(),2 => new Mouse(),3 => new CustomController(),_ => null};
    }
    private static void Require(bool value,string text) {if (!value) throw new InvalidOperationException(text);}
    private static int Main()
    {
        var received=new List<uint>();
        using (var listener=InputSystemFacts.Attach(typeof(InputSystem),received.Add)) {
            Require(listener!=null,"optional Input System listener attached through its observable contract");
            InputSystem.onEvent.Send("DCFG",1);InputSystem.onEvent.Send("STAT",99);
            InputSystem.onEvent.Send("STAT",1);InputSystem.onEvent.Send("DLTA",2);InputSystem.onEvent.Send("STAT",3);
            Require(received.Count==3 && received[0]==1 && received[1]==2 && received[2]==4,"only real state/delta receipt kinds, including controller subclasses");
        }
        InputSystem.onEvent.Send("STAT",1);
        Require(received.Count==3,"listener disposed before integration teardown");
        Require(InputSystemFacts.Attach(typeof(Program),received.Add)==null,"unavailable package is not fabricated as input");
        using (var first=LatencyOwnerLease.TryAcquire()) {
            Require(first!=null,"first orchestrator claims process pacing");
            if (OperatingSystem.IsWindows()) {
                using var second=LatencyOwnerLease.TryAcquire();Require(second==null,"second managed/native-compatible owner cannot pace");
            }
        }
        if (OperatingSystem.IsWindows()) {
            string key="Local\\ReScaleFrame.LatencyOwner."+Environment.ProcessId.ToString(CultureInfo.InvariantCulture);
            using (var native=new EventWaitHandle(false,EventResetMode.ManualReset,key,out bool created)) {
                Require(created,"test models the native core's Win32 event lease");
                using var denied=LatencyOwnerLease.TryAcquire();Require(denied==null,"native-compatible lease excludes managed pacing");
            }
        }
        using var resumed=LatencyOwnerLease.TryAcquire();Require(resumed!=null,"drained orchestrator releases process pacing");
        Console.WriteLine("PASS: Unity keyboard/mouse/controller event receipt and disposal; process pacing ownership exclusion");
        return 0;
    }
}

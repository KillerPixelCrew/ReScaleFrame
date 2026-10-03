// SPDX-License-Identifier: GPL-3.0-only
using System;
using System.Runtime.InteropServices;
using System.Runtime.CompilerServices;
using HarmonyLib;
using ReScaleFrame.Unity;

internal static class Program
{
    public static int Run() => Main();
    [MethodImpl(MethodImplOptions.NoInlining)] private static int Original(int value) => value + 1;
    private static void Result(ref int __result) => __result *= 2;
    private static int Main()
    {
        try
        {
            if (Marshal.SizeOf<CameraFrame>() != 384 || Marshal.SizeOf<Packet>() != 520 ||
                Marshal.SizeOf<Api>() != 56 || Marshal.SizeOf<Configuration>() != 40 ||
                Marshal.OffsetOf<Packet>(nameof(Packet.Camera)).ToInt32() != 72)
                throw new InvalidOperationException("Managed/native packet layout differs.");
            var methods = UrpAdapter.Contracts();
            if (methods.Length != 9) throw new InvalidOperationException("Incomplete URP contract.");
            foreach (var method in methods) Console.WriteLine("Matched " + method.DeclaringType.FullName + "." + method.Name);
            if (Type.GetType("Mono.Runtime") == null)
            {
                Console.WriteLine("PASS: layout and shipped URP metadata. Run the Mono fixture to exercise the deployed Harmony assembly.");
                return 0;
            }
            // Exercise the deployed fat Harmony assembly in Mono, not the CoreCLR reference facade.
            const string owner = "org.killerpixelcrew.rescaleframe.contract-test";
            var harmony = new Harmony(owner);
            harmony.Patch(typeof(Program).GetMethod(nameof(Original), System.Reflection.BindingFlags.NonPublic | System.Reflection.BindingFlags.Static),
                postfix: new HarmonyMethod(typeof(Program), nameof(Result)));
            if (Original(3) != 8) throw new InvalidOperationException("Harmony patch did not execute.");
            harmony.UnpatchAll(owner);
            if (Original(3) != 4) throw new InvalidOperationException("Harmony cleanup failed.");
            Console.WriteLine("PASS: packet layout, complete shipped URP contract, Harmony patch and cleanup. No Unity render execution.");
            return 0;
        }
        catch (Exception error) { Console.Error.WriteLine(error); return 1; }
    }
}

// SPDX-License-Identifier: GPL-3.0-only
using System;
using System.Runtime.InteropServices;
using System.Runtime.CompilerServices;
using HarmonyLib;
using ReScaleFrame.Unity;
using UnityEngine.Rendering;
using UnityEngine.Rendering.RenderGraphModule;
using UnityEngine.Experimental.Rendering;

internal static class Program
{
    public static int Run() => Main();
    [MethodImpl(MethodImplOptions.NoInlining)] private static int Original(int value) => value + 1;
    private static void Result(ref int __result) => __result *= 2;
    private static void ImportedBackbufferSnapshot()
    {
        // Exercise the shipped registry's imported-backbuffer branch without creating a Unity
        // graphics device. Only graph/resource metadata is populated; no renderer is simulated.
        var fields = System.Reflection.BindingFlags.Public | System.Reflection.BindingFlags.NonPublic | System.Reflection.BindingFlags.Instance;
        var assembly = typeof(RenderGraph).Assembly;
        var registryType = assembly.GetType("UnityEngine.Rendering.RenderGraphModule.RenderGraphResourceRegistry", true);
        var dataType = registryType.GetNestedType("RenderGraphResourcesData", System.Reflection.BindingFlags.NonPublic);
#pragma warning disable SYSLIB0050
        var registry = System.Runtime.Serialization.FormatterServices.GetUninitializedObject(registryType);
        var graph = (RenderGraph)System.Runtime.Serialization.FormatterServices.GetUninitializedObject(typeof(RenderGraph));
        var target = (RTHandle)System.Runtime.Serialization.FormatterServices.GetUninitializedObject(typeof(RTHandle));
#pragma warning restore SYSLIB0050
        typeof(RTHandle).GetField("m_NameID", fields).SetValue(target, new RenderTargetIdentifier(BuiltinRenderTextureType.CameraTarget));
        var textureType = assembly.GetType("UnityEngine.Rendering.RenderGraphModule.TextureResource", true);
        var texture = Activator.CreateInstance(textureType, true);
        textureType.GetField("imported", fields).SetValue(texture, true);
        textureType.GetField("validDesc", fields).SetValue(texture, false);
        textureType.GetField("graphicsResource", fields).SetValue(texture, target);
        textureType.GetField("desc", fields).SetValue(texture, new TextureDesc(1920, 1200) { format = GraphicsFormat.B8G8R8A8_UNorm });
        var data = Activator.CreateInstance(dataType, true);
        var resources = dataType.GetField("resourceArray", fields).GetValue(data);
        resources.GetType().GetMethod("Resize").Invoke(resources, new object[] { 2, true });
        ((Array)resources.GetType().GetField("m_Array", fields).GetValue(resources)).SetValue(texture, 1);
        var allData = Array.CreateInstance(dataType, 3); allData.SetValue(data, 0);
        registryType.GetField("m_RenderGraphResources", fields).SetValue(registry, allData);
        typeof(RenderGraph).GetField("m_Resources", fields).SetValue(graph, registry);
        var handle = (TextureHandle)Activator.CreateInstance(typeof(TextureHandle), fields, null, new object[] { 1, false, true }, null);
        bool rejected = false;
        try { graph.GetTextureDesc(handle); } catch (ArgumentException) { rejected = true; }
        if (!rejected) throw new InvalidOperationException("Shipped imported backbuffer must reject GetTextureDesc.");
        if (Type.GetType("Mono.Runtime") != null) {
            Console.WriteLine("PASS: shipped imported backbuffer rejects GetTextureDesc.");
            Console.WriteLine("SKIP: supported render-target lookup needs Unity native Object calls unavailable in standalone Mono.");
            return;
        }
        var descriptor = UrpAdapter.SceneSnapshotDescriptor(graph, handle);
        if (descriptor.width != 1920 || descriptor.height != 1200 || descriptor.slices != 1 ||
            descriptor.dimension != TextureDimension.Tex2D || descriptor.format != GraphicsFormat.B8G8R8A8_UNorm)
            throw new InvalidOperationException("Imported snapshot metadata differs from its render target.");
        Console.WriteLine("PASS: shipped imported backbuffer rejects GetTextureDesc; supported snapshot metadata succeeds.");
    }
    private static int Main()
    {
        try
        {
            if (Marshal.SizeOf<CameraFrame>() != 384 || Marshal.SizeOf<Packet>() != 520 ||
                Marshal.SizeOf<Api>() != 64 || Marshal.SizeOf<Configuration>() != 40 ||
                Marshal.OffsetOf<Packet>(nameof(Packet.Camera)).ToInt32() != 72)
                throw new InvalidOperationException("Managed/native packet layout differs.");
            var methods = UrpAdapter.Contracts();
            if (methods.Length != 12) throw new InvalidOperationException("Incomplete URP contract.");
            foreach (var method in methods) Console.WriteLine("Matched " + method.DeclaringType.FullName + "." + method.Name);
            ImportedBackbufferSnapshot();
            // No snapshot means no native packet. In particular, a missing owned texture must
            // never request a later lookup of the swapchain buffer as the HUD-less source.
            var flags = System.Reflection.BindingFlags.NonPublic | System.Reflection.BindingFlags.Static;
            var bootstrapState = typeof(Bootstrap).GetField("state", flags);
            int previousState = (int)bootstrapState.GetValue(null);
            var previousEnqueue = Native.Enqueue;
            int importedPackets = 0;
            try {
                bootstrapState.SetValue(null, 2);
                Native.Enqueue = (ref Packet packet) => {
                    if (packet.Flags != (PacketFlags.Hudless | PacketFlags.Probe) || packet.Color != IntPtr.Zero) throw new InvalidOperationException("Imported-buffer packet changed.");
                    ++importedPackets; return IntPtr.Zero;
                };
                var passType = typeof(UrpAdapter).GetNestedType("PassData", System.Reflection.BindingFlags.NonPublic);
                var pass = Activator.CreateInstance(passType, true);
                passType.GetField("Packet", System.Reflection.BindingFlags.NonPublic | System.Reflection.BindingFlags.Instance)
                    .SetValue(pass, new Packet { Flags = PacketFlags.Hudless | PacketFlags.Probe });
                var execute = typeof(UrpAdapter).GetMethod("Execute", flags);
                execute.Invoke(null, new[] { pass, Activator.CreateInstance(execute.GetParameters()[1].ParameterType) });
                if (importedPackets != 0) throw new InvalidOperationException("HUD-less work without an owned snapshot reached native resolution.");
            }
            finally { Native.Enqueue = previousEnqueue; bootstrapState.SetValue(null, previousState); }
            Console.WriteLine("PASS: HUD-less work requires a graph-owned snapshot.");
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

// SPDX-License-Identifier: GPL-3.0-only
using System;
using System.Reflection;
using System.Threading;
using HarmonyLib;
using UnityEngine;
using UnityEngine.Rendering;
using UnityEngine.Rendering.Universal;

namespace ReScaleFrame.Unity
{
    public static class Bootstrap
    {
        internal const string Owner = "org.killerpixelcrew.rescaleframe.unity-mono";
        internal static readonly Harmony Harmony = new Harmony(Owner);
        private static int state;
        private static int mainThread;
        private static int producers;
        // A producer registers before it looks at the state and Stop publishes the state before it
        // looks at the producers; both sides use full fences, so one of them always sees the other.
        // A spurious count only makes Stop report busy, and the host retries.
        internal struct Producer : IDisposable
        {
            internal bool Valid;
            public void Dispose() { if (Valid) { Interlocked.Decrement(ref producers); Valid = false; } }
        }
        internal static Producer EnterProducer()
        {
            Interlocked.Increment(ref producers);
            if (Volatile.Read(ref state) == 2) return new Producer { Valid = true };
            Interlocked.Decrement(ref producers);
            return new Producer();
        }

        // Called on an attached Mono thread. Unity objects are accessed only after its render loop.
        public static int Start(IntPtr nativeApi)
        {
            if (Interlocked.CompareExchange(ref state, 1, 0) != 0) return -1;
            try
            {
                Native.Initialize(nativeApi);
                Native.ReportState(1);
                MethodInfo boundary = AccessTools.Method(typeof(RenderPipelineManager), "DoRenderLoop_Internal");
                if (boundary == null || boundary.GetMethodBody() == null)
                    throw new MissingMethodException("Managed render-loop boundary unavailable.");
                Harmony.Patch(boundary, postfix: new HarmonyMethod(typeof(Bootstrap), nameof(RenderLoop)));
                Native.Log("Mono helper loaded; awaiting Unity main-thread render loop.");
                return 0;
            }
            catch (Exception error)
            {
                Native.Log?.Invoke("Mono bootstrap refused: " + error);
                Native.ReportState?.Invoke(3);
                Harmony.UnpatchAll(Owner);
                state = 0;
                return -1;
            }
        }

        private static void RenderLoop()
        {
            Interlocked.Increment(ref producers);
            if (Volatile.Read(ref state) != 1)
            {
                Interlocked.Decrement(ref producers);
                return;
            }
            try
            {
                if (!(GraphicsSettings.currentRenderPipeline is UniversalRenderPipelineAsset)) return;
                if (!Application.unityVersion.StartsWith("6000.3.", StringComparison.Ordinal) ||
                    SystemInfo.graphicsDeviceType != GraphicsDeviceType.Direct3D12)
                    throw new NotSupportedException("Initial adapter requires Unity 6000.3 URP on native D3D12.");
                mainThread = Thread.CurrentThread.ManagedThreadId;
                Native.Api.RenderEvent = Native.rsf_unity_get_render_event();
                if (Native.Api.RenderEvent == IntPtr.Zero)
                    throw new NotSupportedException("Unity did not register its D3D12 native interface.");
                UrpAdapter.Install();
                CpuBoundaries.Install();
                if (Interlocked.CompareExchange(ref state, 2, 1) != 1) return;
                Native.ReportState(2);
                Native.Log("Unity main-thread URP adapter installed; waiting for native D3D12 provider plan.");
            }
            catch (Exception error)
            {
                Volatile.Write(ref state, 3);
                Native.Log("Unity adapter refused: " + error);
                Native.ReportState(3);
                Harmony.UnpatchAll(Owner);
            }
            finally { Interlocked.Decrement(ref producers); }
        }

        internal static bool OnMainThread => Volatile.Read(ref state) == 2 && Thread.CurrentThread.ManagedThreadId == mainThread;
        public static int Stop()
        {
            Interlocked.Exchange(ref state, 4);
            if (Volatile.Read(ref producers) != 0) return -1;
            Harmony.UnpatchAll(Owner);
            UrpAdapter.Clear();
            CpuBoundaries.Clear();
            Native.Log?.Invoke("Unity managed producers stopped; native queued work must drain.");
            Native.ReportState?.Invoke(4);
            state = 0;
            return 0;
        }
    }
}

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
        private static string failure;
        private static readonly object producerGate = new object();
        private static int producers;
        internal sealed class Producer : IDisposable
        {
            internal bool Valid;
            public void Dispose() { if (Valid) { lock (producerGate) --producers; Valid = false; } }
        }
        internal static Producer EnterProducer()
        {
            lock (producerGate)
            {
                bool valid = state == 2;
                if (valid) ++producers;
                return new Producer { Valid = valid };
            }
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
                failure = error.ToString();
                Native.Log?.Invoke("Mono bootstrap refused: " + failure);
                Native.ReportState?.Invoke(3);
                Harmony.UnpatchAll(Owner);
                state = 0;
                return -1;
            }
        }

        private static void RenderLoop()
        {
            lock (producerGate) { if (state != 1) return; ++producers; }
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
                lock (producerGate) { if (state == 4) return; state = 2; }
                Native.ReportState(2);
                Native.Log("Unity main-thread URP adapter installed; waiting for native D3D12 provider plan.");
            }
            catch (Exception error)
            {
                failure = error.ToString();
                Volatile.Write(ref state, 3);
                Native.Log("Unity adapter refused: " + failure);
                Native.ReportState(3);
                Harmony.UnpatchAll(Owner);
            }
            finally { lock (producerGate) --producers; }
        }

        internal static bool OnMainThread => Volatile.Read(ref state) == 2 && Thread.CurrentThread.ManagedThreadId == mainThread;
        public static int Stop()
        {
            lock (producerGate) { state = 4; if (producers != 0) return -1; }
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

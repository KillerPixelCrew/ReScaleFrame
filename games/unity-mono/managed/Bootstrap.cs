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
    /// <summary>Loads the managed adapter on Unity's main thread and owns its producer lifecycle.</summary>
    public static class Bootstrap
    {
        internal const string Owner = "org.killerpixelcrew.rescaleframe.unity-mono";
        internal static readonly Harmony Harmony = new Harmony(Owner);
        // 0 inert, 1 waiting for main loop, 2 installed, 3 refused, 4 stopping.
        // producerGate serializes admission with Stop; reports use the native bridge's matching stages.
        private static int state;
        private static int mainThread;
        private static string failure;
        private static readonly object producerGate = new object();
        private static int producers;
        /// <summary>One admitted callback; disposal releases its producer count exactly once.</summary>
        internal sealed class Producer : IDisposable
        {
            internal bool Valid;
            // Pair every admitted callback with a decrement so Stop can wait for producers to drain.
            public void Dispose() { if (Valid) { lock (producerGate) --producers; Valid = false; } }
        }
        /// <summary>Admit work only in the installed state while synchronizing with Stop.</summary>
        internal static Producer EnterProducer()
        {
            lock (producerGate)
            {
                bool valid = state == 2;
                if (valid) ++producers;
                return new Producer { Valid = valid };
            }
        }

        /// <summary>Initialize callbacks and patch the managed loop from an owned Mono-attached thread.</summary>
        /// <param name="nativeApi">Borrowed native API address; its contents are copied before return.</param>
        /// <returns>Zero after bootstrap installation, -1 on duplicate admission or failure.</returns>
        /// <remarks>Unity object access is deferred to RenderLoop on the player's main thread.</remarks>
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

        /// <summary>Activate supported URP/D3D12 adapters on the first eligible main-thread loop.</summary>
        /// <remarks>A pipeline/version/device refusal removes this owner's Harmony patches and reports stage 3.</remarks>
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

        /// <summary>Whether this callback may access eligible camera state on the installed main thread.</summary>
        internal static bool OnMainThread => Volatile.Read(ref state) == 2 && Thread.CurrentThread.ManagedThreadId == mainThread;
        /// <summary>Close admission, unpatch owned producers and clear managed adapter state.</summary>
        /// <returns>Zero after cleanup, -1 while an admitted producer still needs to leave.</returns>
        /// <remarks>Retry a busy stop while keeping native callbacks loaded; native GPU drainage is separate.</remarks>
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

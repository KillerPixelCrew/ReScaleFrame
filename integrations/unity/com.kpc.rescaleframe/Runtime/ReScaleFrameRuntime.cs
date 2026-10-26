// SPDX-License-Identifier: GPL-3.0-only
#if ENABLE_UPSCALER_FRAMEWORK
using System;
using System.Collections.Generic;
using System.IO;
using System.Runtime.InteropServices;
using UnityEngine;
using UnityEngine.LowLevel;
using UnityEngine.PlayerLoop;
using UnityEngine.Rendering;
using UnityEngine.Rendering.Universal;

namespace ReScaleFrame
{
    public static class ReScaleFrameRuntime
    {
        private struct BeforeInput { }
        private struct AfterSimulation { }
        private static bool ready, failed;
        private static Native.Settings previous;
        private static Native.Settings rejected;
        private static ReScaleFrameOptions active;
        private static RenderTexture editorTarget, composedCopy;
        private static Native.Packet completedFrame;
        private static ulong submittedFrame;
        private static bool previewVisible;
        private static double fpsEpoch;
        private static int fpsBaseFrame, fpsSampledFrame = -1;
        private static ulong fpsPresented, fpsProvider;
        private static ReScaleFrameFrameRates frameRates;
        public static bool IsReady => ready;
        public static string LastError { get; private set; }
        public static ReScaleFrameOptions Options => active;
        public static string ProjectId { get; set; } = Application.cloudProjectId;
        public static string RuntimeDirectory { get; set; } = Path.Combine(Application.streamingAssetsPath, "ReScaleFrame");

        [RuntimeInitializeOnLoadMethod(RuntimeInitializeLoadType.SubsystemRegistration)]
        private static void Reset() {
            ready = failed = false; active = null; previous = rejected = default; LastError = null;
            completedFrame = default; submittedFrame = 0; fpsEpoch = 0; fpsSampledFrame = -1;
            Register(); RemoveLoop();
        }
        internal static void Register() { UpscalerRegistry.Register<ReScaleFrameUpscaler, ReScaleFrameOptions>(ReScaleFrameUpscaler.Name); }
        public static void Apply(ReScaleFrameOptions options) {
            if (options == null) throw new ArgumentNullException(nameof(options));
            active = options;
            if (!Application.isPlaying || failed) return;
            if (Application.isEditor && !Native.HasPreviewApi) { LastError = "Restart Unity to load the updated ReScaleFrame native presentation host."; return; }
            try {
                if (!ready) {
                    if (string.IsNullOrEmpty(ProjectId)) throw new InvalidOperationException("Set ReScaleFrameRuntime.ProjectId to the project's GUID before initialization.");
                    ready = Native.Start(RuntimeDirectory, ProjectId);
                    if (!ready && Native.InitializationResult == 2) { LastError = "Waiting for prior GPU work to finish."; return; }
                    if (!ready) throw new InvalidOperationException("The Unity graphics device did not initialize the ReScaleFrame bridge.");
                    Native.rsfu_surface((uint)Screen.width, (uint)Screen.height);
                    InstallLoop(); Native.State(2); Application.quitting += Stop;
                }
                var snapshot = options.Snapshot;
                if (!snapshot.Equals(previous)) {
                    if (Native.rsfu_configure(ref snapshot) == 0) {
                        LastError = "ReScaleFrame refused the requested configuration; the previous configuration is retained.";
                        if (!snapshot.Equals(rejected)) Debug.LogWarning(LastError);
                        rejected = snapshot; return;
                    }
                    previous = snapshot;
                    rejected = default; LastError = null;
                }
            } catch (Exception error) {
                LastError = error.Message; failed = true;
                Debug.LogWarning("ReScaleFrame: " + LastError);
            }
        }
        public static void SetGameplay(bool gameplay) {
            if (active != null) { active.gameplay = gameplay; Apply(active); }
        }
        public static bool TryGetStatus(out ReScaleFrameStatus status) {
            status = default;
            if (!ready) return false;
            var native = new Native.Status { Size = (uint)Marshal.SizeOf<Native.Status>() };
            if (Native.rsfu_status(ref native) == 0) return false;
            status = new ReScaleFrameStatus(native); return true;
        }
        public static void ConfigureEditorPreview(RenderTexture target, Rect pixels, bool visible) {
            editorTarget = target; previewVisible = visible && target != null;
            if (Native.HasPreviewApi) Native.rsfu_preview_region(Mathf.RoundToInt(pixels.x), Mathf.RoundToInt(pixels.y),
                Mathf.RoundToInt(pixels.width), Mathf.RoundToInt(pixels.height), previewVisible ? 1u : 0u);
        }
        internal static void CompletedFrame(Native.Packet packet) { completedFrame = packet; }
        internal static void PresentCompletedFrame() {
            if (!ready || completedFrame.Frame != Native.FrameId || submittedFrame == completedFrame.Frame) return;
            var packet = completedFrame; packet.Flags = 2 | 4;
            packet.Color = packet.Depth = packet.Motion = packet.Output = IntPtr.Zero;
            var commands = CommandBufferPool.Get("ReScaleFrame completed frame");
            if (Application.isEditor) {
                if (!previewVisible || editorTarget == null || !editorTarget.IsCreated()) { CommandBufferPool.Release(commands); return; }
                if (composedCopy == null || composedCopy.width != editorTarget.width || composedCopy.height != editorTarget.height) {
                    if (composedCopy != null) UnityEngine.Object.Destroy(composedCopy);
                    var descriptor = new RenderTextureDescriptor(editorTarget.width, editorTarget.height,
                        UnityEngine.Experimental.Rendering.GraphicsFormat.R8G8B8A8_UNorm, 0) { msaaSamples = 1 };
                    composedCopy = new RenderTexture(descriptor) { name = "ReScaleFrame completed Game view", hideFlags = HideFlags.HideAndDontSave };
                    composedCopy.Create();
                }
                commands.SetRenderTarget(composedCopy);
                commands.SetViewport(new Rect(0, 0, composedCopy.width, composedCopy.height));
                Blitter.BlitTexture(commands, new RenderTargetIdentifier(editorTarget), new Vector4(1, 1, 0, 0), ReScaleFrameUpscaler.CopyMaterial, 2);
                commands.SetRenderTarget(composedCopy);
                packet.Color = composedCopy.GetNativeTexturePtr();
            }
            IntPtr queued = Native.Enqueue(ref packet);
            if (queued != IntPtr.Zero) {
                commands.IssuePluginEventAndData(Native.RenderEvent, Native.EventId + 1, queued);
                Graphics.ExecuteCommandBuffer(commands); submittedFrame = packet.Frame;
            }
            CommandBufferPool.Release(commands);
        }
        public static bool TryGetFrameRates(out ReScaleFrameFrameRates rates) {
            rates = frameRates;
            if (!Application.isPlaying) return false;
            if (fpsSampledFrame == Time.frameCount) return true;
            fpsSampledFrame = Time.frameCount;
            var counters = new Native.FrameCounters { Size = (uint)Marshal.SizeOf<Native.FrameCounters>() };
            bool valid = ready && Native.HasPreviewApi && Native.rsfu_frame_counters(ref counters) != 0 && counters.Valid != 0;
            double now = Time.realtimeSinceStartupAsDouble;
            if (fpsEpoch == 0 || (valid && fpsProvider != counters.ProviderGeneration) || Time.frameCount < fpsBaseFrame) {
                fpsEpoch = now; fpsBaseFrame = Time.frameCount; fpsPresented = counters.TotalPresented; fpsProvider = counters.ProviderGeneration;
                frameRates = default; rates = frameRates; return true;
            }
            double elapsed = now - fpsEpoch;
            if (elapsed >= 0.5) {
                bool outputValid = valid && counters.TotalPresented >= fpsPresented;
                double output = outputValid ? (counters.TotalPresented - fpsPresented) / elapsed : 0;
                frameRates = new ReScaleFrameFrameRates((Time.frameCount - fpsBaseFrame) / elapsed, output, outputValid);
                fpsEpoch = now; fpsBaseFrame = Time.frameCount; fpsPresented = counters.TotalPresented;
            }
            rates = frameRates; return true;
        }
        private static void Begin() {
            if (!ready || (Application.isEditor && !previewVisible)) return;
            ulong frame = Native.FrameId;
            Native.rsfu_surface((uint)Screen.width, (uint)Screen.height);
            Native.Cpu(0, frame); Native.Cpu(6, frame); Native.Cpu(1, frame); Native.Cpu(2, frame);
            Native.rsfu_input(frame, 0);
        }
        public static void NotifyInput(uint kinds) {
            if ((kinds & ~7u) != 0) throw new ArgumentOutOfRangeException(nameof(kinds));
            if (ready) Native.rsfu_input(Native.FrameId, kinds);
        }
        private static void End() { if (ready && (!Application.isEditor || previewVisible)) Native.Cpu(3, Native.FrameId); }
        private static void InstallLoop() {
            RemoveLoop();
            var loop = PlayerLoop.GetCurrentPlayerLoop(); var nodes = new List<PlayerLoopSystem>();
            foreach (var node in loop.subSystemList) {
                if (node.type == typeof(EarlyUpdate)) nodes.Add(new PlayerLoopSystem { type = typeof(BeforeInput), updateDelegate = Begin });
                nodes.Add(node);
                if (node.type == typeof(PreLateUpdate)) nodes.Add(new PlayerLoopSystem { type = typeof(AfterSimulation), updateDelegate = End });
            }
            loop.subSystemList = nodes.ToArray(); PlayerLoop.SetPlayerLoop(loop);
        }
        private static void RemoveLoop() {
            var loop = PlayerLoop.GetCurrentPlayerLoop(); if (loop.subSystemList == null) return;
            var nodes = new List<PlayerLoopSystem>();
            foreach (var node in loop.subSystemList) if (node.type != typeof(BeforeInput) && node.type != typeof(AfterSimulation)) nodes.Add(node);
            loop.subSystemList = nodes.ToArray(); PlayerLoop.SetPlayerLoop(loop);
        }
        public static void Stop() {
            RemoveLoop(); Application.quitting -= Stop;
            if (!ready) return;
            Native.State(0); ready = false;
            if (Native.rsfu_shutdown() == 0) Debug.Log("ReScaleFrame: GPU work remains owned until the graphics device drains.");
        }
    }
}
#endif

// SPDX-License-Identifier: GPL-3.0-only
#if ENABLE_UPSCALER_FRAMEWORK
using System;
using UnityEngine;
using UnityEngine.Experimental.Rendering;
using UnityEngine.Rendering;
using UnityEngine.Rendering.RenderGraphModule;
using UnityEngine.Rendering.Universal;

namespace ReScaleFrame
{
    public sealed class ReScaleFrameUpscaler : AbstractUpscaler
    {
        public const string Name = "ReScaleFrame";
        public override string name => Name;
        public override bool isTemporal => true;
        public override bool supportsSharpening => false;
        public override bool hasQualityMode => true;
        private sealed class History : IUpscalerContext {
            public Vector2Int createdForDisplayResolution { get; }
            public int lastUsedFrame { get; set; }
            internal Matrix4x4 PreviousVP;
            internal Vector2 PreviousJitter;
            internal int Frame = -1;
            internal uint Generation;
            internal History(Vector2Int size) { createdForDisplayResolution = size; }
            public bool IsValidForOptions(UpscalerOptions options) => options is ReScaleFrameOptions;
            public void Cleanup(CommandBuffer commands) { Frame = -1; }
        }
        private sealed class PassData {
            internal TextureHandle Color, Depth, Motion, Output, PreparedColor, PreparedDepth, Encoded, Spatial;
            internal Native.Packet Packet;
            internal bool Fsr1;
            internal float Sharpness;
        }
        private static Material copyMaterial;
        internal static Material CopyMaterial => copyMaterial != null ? copyMaterial : copyMaterial =
            CoreUtils.CreateEngineMaterial(Resources.Load<Shader>("ReScaleFrame/Copy"));
        private static Material fsrMaterial;
        private static Material FsrMaterial => fsrMaterial != null ? fsrMaterial : fsrMaterial =
            CoreUtils.CreateEngineMaterial(Resources.Load<Shader>("ReScaleFrame/Fsr1"));
        public override IUpscalerContext CreateContext(UpscalerOptions options, Vector2Int size) => new History(size);
        public override void CalculateJitter(int frame, float ratio, out Vector2 jitter, out bool allowScaling) {
            var options = ReScaleFrameRuntime.Options;
            bool temporal = ReScaleFrameRuntime.IsReady && Native.ProjectionReady && Native.rsfu_temporal_ready() != 0 && options != null &&
                options.superResolution != SuperResolution.Off && options.superResolution != SuperResolution.FSR1;
            int phase = Mathf.Max(1, Mathf.RoundToInt(8 * ratio * ratio));
            int index = (frame % phase) + 1;
            jitter = temporal ? new Vector2(Halton(index, 2) - 0.5f, Halton(index, 3) - 0.5f) : Vector2.zero;
            allowScaling = false;
        }
        private static float Halton(int index, int radix) {
            float value = 0, weight = 1;
            while (index > 0) { weight /= radix; value += weight * (index % radix); index /= radix; }
            return value;
        }
        public override UpscalerResolutionInfo GetResolutionInfo(Vector2Int display, UpscalerOptions options) {
            if (options is not ReScaleFrameOptions settings) return UpscalerResolutionInfo.Fixed(display);
            ReScaleFrameRuntime.Apply(settings);
            if (!Application.isPlaying || !ReScaleFrameRuntime.IsReady) return UpscalerResolutionInfo.Fixed(display);
            if (settings.superResolution == SuperResolution.Off) return UpscalerResolutionInfo.Fixed(display);
            if (ReScaleFrameRuntime.IsReady) {
                var native = Native.GetConfiguration((uint)display.x, (uint)display.y);
                if (native.Enabled != 0 && native.RenderWidth != 0 && native.RenderHeight != 0)
                    return UpscalerResolutionInfo.Fixed(new Vector2Int((int)native.RenderWidth, (int)native.RenderHeight));
            }
            float[] ratios = { 1, 1.5f, 1.7f, 2, 3, 1.3f };
            float ratio = ratios[(int)settings.quality];
            return UpscalerResolutionInfo.Fixed(new Vector2Int(Mathf.Max(1, (int)(display.x / ratio)), Mathf.Max(1, (int)(display.y / ratio))));
        }
        public override void RecordRenderGraph(RenderGraph graph, ContextContainer frameData) {
            var io = frameData.Get<UpscalingIO>();
            var camera = frameData.Get<UniversalCameraData>();
            if (io.options is ReScaleFrameOptions options) ReScaleFrameRuntime.Apply(options);
            if (camera.cameraType != CameraType.Game || io.enableTexArray || !io.cameraColor.IsValid()) return;
            var history = (History)io.context;
            var config = ReScaleFrameRuntime.IsReady ? Native.GetConfiguration((uint)io.postUpscaleResolution.x, (uint)io.postUpscaleResolution.y) : default;
            var descriptor = io.cameraColor.GetDescriptor(graph);
            descriptor.sizeMode = TextureSizeMode.Explicit; descriptor.width = io.postUpscaleResolution.x; descriptor.height = io.postUpscaleResolution.y;
            descriptor.format = GraphicsFormat.R16G16B16A16_SFloat; descriptor.msaaSamples = MSAASamples.None;
            descriptor.enableRandomWrite = true; descriptor.useDynamicScale = false; descriptor.clearBuffer = false;
            descriptor.name = "ReScaleFrame reconstructed scene";
            var output = graph.CreateTexture(descriptor);
            var inputDescriptor = descriptor;
            inputDescriptor.width = io.preUpscaleResolution.x; inputDescriptor.height = io.preUpscaleResolution.y;
            inputDescriptor.enableRandomWrite = false; inputDescriptor.name = "ReScaleFrame linear colour input";
            var preparedColor = graph.CreateTexture(inputDescriptor);
            inputDescriptor.format = GraphicsFormat.R32_SFloat; inputDescriptor.name = "ReScaleFrame raw depth input";
            var preparedDepth = graph.CreateTexture(inputDescriptor);
            bool fsr1 = io.options is ReScaleFrameOptions spatialOptions && spatialOptions.superResolution == SuperResolution.FSR1;
            TextureHandle encoded = TextureHandle.nullHandle, spatial = TextureHandle.nullHandle;
            if (fsr1) {
                inputDescriptor.format = GraphicsFormat.R16G16B16A16_SFloat; inputDescriptor.name = "ReScaleFrame FSR1 perceptual input";
                encoded = graph.CreateTexture(inputDescriptor);
                inputDescriptor.width = descriptor.width; inputDescriptor.height = descriptor.height; inputDescriptor.name = "ReScaleFrame FSR1 EASU output";
                spatial = graph.CreateTexture(inputDescriptor);
            }
            Matrix4x4 projection = GL.GetGPUProjectionMatrix(camera.camera.nonJitteredProjectionMatrix, true);
            Matrix4x4 view = camera.GetViewMatrix(), vp = projection * view;
            bool reset = io.resetHistory || history.Frame != Time.frameCount - 1 || history.Generation != config.Generation;
            Matrix4x4 previous = reset ? vp : history.PreviousVP;
            var packet = new Native.Packet {
                Size = Native.PacketSize, Version = 2, Session = Native.Bridge.Session,
                Frame = Native.FrameId, View = EntityId.ToULong(camera.camera.GetEntityId()), Generation = config.Generation,
                Flags = (reset ? 1u : 0u) | (config.Enabled == 0 ? 2u : 0u),
                Camera = new Native.CameraFrame {
                    Size = Native.CameraSize, Version = 1, Projection = projection, InverseProjection = projection.inverse,
                    ViewToWorld = view.inverse, WorldToView = view, ClipToPrevious = previous * vp.inverse,
                    Jitter = io.subpixelJitter, PreviousJitter = reset ? io.subpixelJitter : history.PreviousJitter,
                    RenderWidth = (uint)io.preUpscaleResolution.x, RenderHeight = (uint)io.preUpscaleResolution.y,
                    OutputWidth = (uint)io.postUpscaleResolution.x, OutputHeight = (uint)io.postUpscaleResolution.y,
                    Near = io.nearClipPlane, Far = io.farClipPlane, Fov = io.fieldOfViewDegrees * Mathf.Deg2Rad,
                    ReversedDepth = io.invertedDepth ? 1u : 0u, MotionJittered = io.jitteredMotionVectors ? 1u : 0u, DeltaTime = io.deltaTime
                }, PreviousToClip = vp * previous.inverse
            };
            history.PreviousVP = vp; history.PreviousJitter = io.subpixelJitter; history.Frame = Time.frameCount; history.Generation = config.Generation;
            frameData.GetOrCreate<ReScaleFrameFrame>().Packet = packet;
            using (var builder = graph.AddUnsafePass<PassData>("ReScaleFrame reconstruction", out var pass)) {
                pass.Color = io.cameraColor; pass.Depth = io.cameraDepth; pass.Motion = io.motionVectorColor; pass.Output = output; pass.Packet = packet;
                pass.PreparedColor = preparedColor; pass.PreparedDepth = preparedDepth;
                pass.Fsr1 = fsr1; pass.Encoded = encoded; pass.Spatial = spatial;
                pass.Sharpness = io.options is ReScaleFrameOptions sharpening ? sharpening.fsr1Sharpness : 0.2f;
                builder.UseTexture(pass.Color, AccessFlags.Read);
                if (pass.Depth.IsValid()) builder.UseTexture(pass.Depth, AccessFlags.Read);
                if (pass.Motion.IsValid()) builder.UseTexture(pass.Motion, AccessFlags.Read);
                builder.UseTexture(output, AccessFlags.Write); builder.AllowPassCulling(false); builder.AllowGlobalStateModification(true);
                builder.UseTexture(preparedColor, AccessFlags.ReadWrite);
                if (pass.Depth.IsValid()) builder.UseTexture(preparedDepth, AccessFlags.ReadWrite);
                if (fsr1) { builder.UseTexture(encoded, AccessFlags.ReadWrite); builder.UseTexture(spatial, AccessFlags.ReadWrite); }
                builder.SetRenderFunc<PassData>(Execute);
            }
            io.cameraColor = output;
        }
        private static void Execute(PassData data, UnsafeGraphContext context) {
            RTHandle source = data.Color, output = data.Output;
            if (source?.rt == null || output?.rt == null) return;
            var packet = data.Packet;
            var nativeCommands = CommandBufferHelpers.GetNativeCommandBuffer(context.cmd);
            var scale = new Vector4((float)packet.Camera.RenderWidth / source.rt.width, (float)packet.Camera.RenderHeight / source.rt.height, 0, 0);
            RTHandle preparedColor = data.PreparedColor, preparedDepth = data.PreparedDepth;
            context.cmd.SetRenderTarget(preparedColor); context.cmd.SetViewport(new Rect(0, 0, packet.Camera.RenderWidth, packet.Camera.RenderHeight));
            Blitter.BlitTexture(nativeCommands, source, scale, CopyMaterial, 0);
            RTHandle depth = data.Depth.IsValid() ? data.Depth : null;
            if (depth?.rt != null) {
                context.cmd.SetRenderTarget(preparedDepth);
                var depthScale = new Vector4((float)packet.Camera.RenderWidth / depth.rt.width, (float)packet.Camera.RenderHeight / depth.rt.height, 0, 0);
                Blitter.BlitTexture(nativeCommands, depth, depthScale, CopyMaterial, 1);
            }
            context.cmd.SetRenderTarget(output); context.cmd.SetViewport(new Rect(0, 0, packet.Camera.OutputWidth, packet.Camera.OutputHeight));
            if (data.Fsr1) {
                RTHandle encoded = data.Encoded, spatial = data.Spatial;
                context.cmd.SetRenderTarget(encoded); context.cmd.SetViewport(new Rect(0, 0, packet.Camera.RenderWidth, packet.Camera.RenderHeight));
                Blitter.BlitTexture(nativeCommands, source, scale, FsrMaterial, 0);
                FSRUtils.SetEasuConstants(nativeCommands, new Vector2(packet.Camera.RenderWidth, packet.Camera.RenderHeight),
                    new Vector2(encoded.rt.width, encoded.rt.height), new Vector2(packet.Camera.OutputWidth, packet.Camera.OutputHeight));
                context.cmd.SetRenderTarget(spatial); context.cmd.SetViewport(new Rect(0, 0, packet.Camera.OutputWidth, packet.Camera.OutputHeight));
                Blitter.BlitTexture(nativeCommands, encoded, new Vector4(1, 1, 0, 0), FsrMaterial, 1);
                FSRUtils.SetRcasConstantsLinear(nativeCommands, data.Sharpness);
                context.cmd.SetRenderTarget(output);
                Blitter.BlitTexture(nativeCommands, spatial, new Vector4(1, 1, 0, 0), FsrMaterial, 2);
            } else Blitter.BlitTexture(nativeCommands, source, scale, 0, true);
            if (!ReScaleFrameRuntime.IsReady) return;
            packet.Color = preparedColor.rt.GetNativeTexturePtr(); packet.Output = output.rt.GetNativeTexturePtr();
            RTHandle motion = data.Motion.IsValid() ? data.Motion : null;
            if ((packet.Flags & 2) == 0) {
                if (!data.Depth.IsValid() || motion?.rt == null) return;
                packet.Depth = preparedDepth.rt.GetNativeTexturePtr(); packet.Motion = motion.rt.GetNativeTexturePtr();
            }
            IntPtr queued = Native.Enqueue(ref packet);
            if (queued != IntPtr.Zero) context.cmd.IssuePluginEventAndData(Native.RenderEvent, Native.EventId, queued);
        }
    }
}
#endif

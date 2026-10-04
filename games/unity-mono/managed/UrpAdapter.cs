// SPDX-License-Identifier: GPL-3.0-only
using System;
using System.Collections.Generic;
using System.Reflection;
using System.Runtime.InteropServices;
using HarmonyLib;
using UnityEngine;
using UnityEngine.Experimental.Rendering;
using UnityEngine.Rendering;
using UnityEngine.Rendering.RenderGraphModule;
using UnityEngine.Rendering.Universal;

namespace ReScaleFrame.Unity
{
    internal static class UrpAdapter
    {
        private sealed class ViewState
        {
            internal Matrix4x4 PreviousVp;
            internal Vector2 Jitter, PreviousJitter;
            internal int Frame = -1;
            internal uint Generation;
            internal int ReconstructedFrame = -1;
        }
        private sealed class PassData
        {
            internal TextureHandle Color, Depth, Motion, Output;
            internal Packet Packet;
        }
        private static readonly Dictionary<int, ViewState> views = new Dictionary<int, ViewState>();
        private static FieldInfo filter, scaling, rawProjection, antialiasing;
        private static MethodInfo updateResolution;
        private static PropertyInfo frameDataProperty;
        private static PropertyInfo hdrOutputProperty;
        private static int overlayFrame = -1;
        private static int hudlessFrame = -1;
        private static object stpFilter, fsrFilter, upscaling;
        private static uint failures;
        private static uint jitterReports;
        [ThreadStatic] private static bool inputScope;

        internal static void Install()
        {
            MethodInfo[] methods = Contracts();
            updateResolution = methods[0];
            Patch(methods[1], postfix: nameof(StackedCamera));
            Patch(methods[2], prefix: nameof(AdditionalCamera), finalizer: nameof(RestoreAdditionalCamera));
            Patch(methods[3], postfix: nameof(Jitter));
            Patch(methods[4], prefix: nameof(Reconstruct));
            Patch(methods[5], prefix: nameof(Probe));
            Patch(methods[6], postfix: nameof(CameraCreated));
            Patch(methods[7], postfix: nameof(Overlay));
            Patch(methods[8], postfix: nameof(AdaptiveCamera));
            Patch(methods[9], prefix: nameof(Hudless));
            Patch(methods[10], prefix: nameof(InputScope), finalizer: nameof(RestoreInputScope));
            Patch(methods[11], prefix: nameof(RequireMotion));
        }

        // Pure metadata preflight. Resolve the complete contract before installing any patch.
        internal static MethodInfo[] Contracts()
        {
            var pipeline = typeof(UniversalRenderPipeline);
            var camera = typeof(UniversalCameraData);
            var recorder = pipeline.Assembly.GetType("UnityEngine.Rendering.Universal.PostProcessPassRenderGraph", true);
            filter = RequireField(camera, "upscalingFilter");
            scaling = RequireField(camera, "imageScalingMode");
            rawProjection = RequireField(camera, "m_ProjectionMatrix");
            antialiasing = RequireField(camera, "antialiasing");
            frameDataProperty = AccessTools.Property(typeof(ScriptableRenderer), "frameData") ?? throw new MissingMemberException("ScriptableRenderer.frameData");
            hdrOutputProperty = AccessTools.Property(camera, "isHDROutputActive") ?? throw new MissingMemberException("UniversalCameraData.isHDROutputActive");
            stpFilter = Enum.Parse(filter.FieldType, "STP");
            fsrFilter = Enum.Parse(filter.FieldType, "FSR");
            upscaling = Enum.Parse(scaling.FieldType, "Upscaling");
            return new[] {
                RequireMethod(recorder, "UpdateCameraResolution", typeof(RenderGraph), camera, typeof(Vector2Int)),
                RequireMethod(pipeline, "InitializeStackedCameraData", typeof(Camera), typeof(UniversalAdditionalCameraData), camera),
                RequireMethod(pipeline, "InitializeAdditionalCameraData", typeof(Camera), typeof(UniversalAdditionalCameraData), typeof(bool), typeof(bool), camera),
                RequireMethod(typeof(TemporalAA), "CalculateJitterMatrix", camera, typeof(TemporalAA).GetNestedType("JitterFunc", BindingFlags.NonPublic)),
                RequireMethod(recorder, "RenderSTP", typeof(RenderGraph), typeof(UniversalResourceData), camera,
                    typeof(TextureHandle).MakeByRefType(), typeof(TextureHandle).MakeByRefType()),
                RequireMethod(recorder, "RenderPostProcessingRenderGraph", typeof(RenderGraph), typeof(ContextContainer),
                    typeof(TextureHandle).MakeByRefType(), typeof(TextureHandle).MakeByRefType(), typeof(TextureHandle).MakeByRefType(),
                    typeof(TextureHandle).MakeByRefType(), typeof(bool), typeof(bool), typeof(bool)),
                RequireMethod(pipeline, "CreateCameraData", typeof(ContextContainer), typeof(Camera), typeof(UniversalAdditionalCameraData)),
                RequireMethod(typeof(UniversalRenderer), "OnAfterRendering", typeof(RenderGraph), typeof(bool)),
                RequireMethod(pipeline, "ApplyAdaptivePerformance", camera),
                RequireMethod(pipeline.Assembly.GetType("UnityEngine.Rendering.Universal.DrawScreenSpaceUIPass", true),
                    "RenderOverlay", typeof(RenderGraph), typeof(ContextContainer), typeof(TextureHandle).MakeByRefType(), typeof(TextureHandle).MakeByRefType()),
                RequireMethod(typeof(UniversalRenderer), "OnRecordRenderGraph", typeof(RenderGraph), typeof(ScriptableRenderContext)),
                RequireMethod(typeof(UniversalRenderer), "GetRenderPassInputs", typeof(bool), typeof(bool), typeof(bool), typeof(bool),
                    typeof(List<ScriptableRenderPass>), pipeline.Assembly.GetType("UnityEngine.Rendering.Universal.MotionVectorRenderPass", true))
            };
        }

        private static FieldInfo RequireField(Type owner, string name) =>
            AccessTools.Field(owner, name) ?? throw new MissingFieldException(owner.FullName, name);
        private static MethodInfo RequireMethod(Type owner, string name, params Type[] arguments)
        {
            MethodInfo method = AccessTools.Method(owner, name, arguments);
            if (method == null || method.GetMethodBody() == null)
                throw new MissingMethodException(owner.FullName, name);
            return method;
        }
        private static void Patch(MethodInfo original, string prefix = null, string postfix = null, string finalizer = null)
        {
            var p = prefix == null ? null : new HarmonyMethod(typeof(UrpAdapter), prefix);
            var q = postfix == null ? null : new HarmonyMethod(typeof(UrpAdapter), postfix);
            var f = finalizer == null ? null : new HarmonyMethod(typeof(UrpAdapter), finalizer);
            Bootstrap.Harmony.Patch(original, prefix: p, postfix: q, finalizer: f);
            Native.Log("URP method matched: " + original.DeclaringType.FullName + "." + original.Name);
        }
        private static bool Eligible(Camera camera, UniversalAdditionalCameraData additional = null)
        {
            return Bootstrap.OnMainThread && camera != null && camera == Camera.main &&
                camera.cameraType == CameraType.Game && camera.targetTexture == null &&
                !camera.orthographic && !camera.stereoEnabled && !camera.allowDynamicResolution &&
                camera.rect == new Rect(0, 0, 1, 1) && (additional == null ||
                    (additional.renderType == CameraRenderType.Base && additional.cameraStack.Count == 0 && additional.renderPostProcessing));
        }
        private static void InputScope(UniversalRenderer __instance, out bool __state)
        {
            __state = inputScope;
            var frame = (ContextContainer)frameDataProperty.GetValue(__instance);
            inputScope = Eligible(frame.Get<UniversalCameraData>().camera);
        }
        private static Exception RestoreInputScope(bool __state, Exception __exception)
        {
            inputScope = __state; return __exception;
        }
        private static void RequireMotion(ref bool isTemporalAAEnabled)
        {
            // This argument is only the input planner's request. Camera AA and jitter policy
            // remain untouched; its existing true branch schedules depth and motion producers.
            if (inputScope) isTemporalAAEnabled = true;
        }
        private static void StackedCamera(Camera baseCamera, UniversalAdditionalCameraData baseAdditionalCameraData, UniversalCameraData cameraData)
        {
            using (var producer = Bootstrap.EnterProducer()) {
            if (!producer.Valid) return;
            if (!Eligible(baseCamera, baseAdditionalCameraData)) return;
            Configuration config = Native.Configuration(baseCamera);
            if (config.Enabled == 0 || config.RenderWidth == 0 || config.RenderHeight == 0) return;
            var descriptor = cameraData.cameraTargetDescriptor;
            descriptor.width = (int)config.RenderWidth; descriptor.height = (int)config.RenderHeight;
            descriptor.msaaSamples = 1; descriptor.useDynamicScale = false;
            cameraData.cameraTargetDescriptor = descriptor;
            cameraData.scaledWidth = descriptor.width; cameraData.scaledHeight = descriptor.height;
            // URP also truncates pixel dimensions multiplied by this scalar for intermediate buffers.
            // Keep those allocations at least as large as both exact vendor dimensions.
            cameraData.renderScale = Mathf.Max((config.RenderWidth + 0.01f) / config.OutputWidth,
                (config.RenderHeight + 0.01f) / config.OutputHeight);
            filter.SetValue(cameraData, config.Backend == 6 ? fsrFilter : stpFilter);
            scaling.SetValue(cameraData, upscaling);
            if (config.Backend != 6) antialiasing.SetValue(cameraData, AntialiasingMode.TemporalAntiAliasing);
            }
        }
        private static void CameraCreated(Camera camera, UniversalAdditionalCameraData additionalCameraData, UniversalCameraData __result)
        {
            // CreateCameraData computes dimensions and MSAA again after InitializeStackedCameraData.
            // Commit the vendor's exact width/height after that computation, before any allocation.
            StackedCamera(camera, additionalCameraData, __result);
        }
        private static void AdaptiveCamera(UniversalCameraData cameraData)
        {
            cameraData.camera.TryGetComponent<UniversalAdditionalCameraData>(out var additional);
            StackedCamera(cameraData.camera, additional, cameraData);
        }
        private static void AdditionalCamera(Camera camera, UniversalAdditionalCameraData additionalCameraData, out AntialiasingMode? __state)
        {
            __state = null;
            using (var producer = Bootstrap.EnterProducer()) {
            if (!producer.Valid) return;
            if (!Eligible(camera, additionalCameraData)) return;
            Configuration config = Native.Configuration(camera);
            if (config.Enabled == 0 || config.Backend == 6 || additionalCameraData == null) return;
            __state = additionalCameraData.antialiasing;
            additionalCameraData.antialiasing = AntialiasingMode.TemporalAntiAliasing;
            }
        }
        private static Exception RestoreAdditionalCamera(UniversalAdditionalCameraData additionalCameraData, AntialiasingMode? __state, Exception __exception)
        {
            if (__state.HasValue && additionalCameraData != null) additionalCameraData.antialiasing = __state.Value;
            return __exception;
        }
        private static ViewState State(Camera camera)
        {
            int id = camera.GetInstanceID();
            if (!views.TryGetValue(id, out ViewState state))
            {
                if (views.Count >= 16) views.Clear();
                state = new ViewState(); views.Add(id, state);
            }
            return state;
        }
        private static void Jitter(UniversalCameraData cameraData, Matrix4x4 __result)
        {
            using (var producer = Bootstrap.EnterProducer()) {
            if (!producer.Valid) return;
            if (!Eligible(cameraData.camera)) return;
            ViewState state = State(cameraData.camera);
            state.Jitter = new Vector2(__result.m03 * cameraData.cameraTargetDescriptor.width * 0.5f,
                __result.m13 * cameraData.cameraTargetDescriptor.height * 0.5f);
            }
        }
        private static Packet Snapshot(UniversalCameraData data, bool probe)
        {
            var camera = data.camera;
            ViewState state = State(camera);
            Configuration config = Native.Configuration(camera);
            Matrix4x4 projection = GL.GetGPUProjectionMatrix((Matrix4x4)rawProjection.GetValue(data), true);
            Matrix4x4 rasterProjection = GL.GetGPUProjectionMatrix(data.GetProjectionMatrix(), true);
            Matrix4x4 shift = rasterProjection * projection.inverse;
            // Measure the translation actually applied after Unity's GPU projection conversion.
            Vector2 rasterJitter = new Vector2(shift.m03 * data.cameraTargetDescriptor.width * 0.5f,
                -shift.m13 * data.cameraTargetDescriptor.height * 0.5f);
            if (!probe && jitterReports++ < 12) Native.Log("Unity raster jitter=" + rasterJitter + " projection jitter=" + state.Jitter);
            Matrix4x4 view = data.GetViewMatrix();
            Matrix4x4 vp = projection * view;
            bool reset = state.Frame != Time.frameCount - 1 || state.Generation != config.Generation;
            Matrix4x4 previous = reset ? vp : state.PreviousVp;
            var packet = new Packet {
                Size = (uint)Marshal.SizeOf<Packet>(), Version = 1, Session = Native.Api.Session,
                Frame = (ulong)(uint)Time.frameCount + 1, View = (ulong)(uint)camera.GetInstanceID(),
                Generation = config.Generation, Flags = probe ? 2u : reset ? 1u : 0u,
                Camera = new CameraFrame {
                    Size = (uint)Marshal.SizeOf<CameraFrame>(), Version = 1,
                    Projection = projection, InverseProjection = projection.inverse,
                    ViewToWorld = view.inverse, WorldToView = view, ClipToPrevious = previous * vp.inverse,
                    Jitter = rasterJitter, PreviousJitter = reset ? rasterJitter : state.PreviousJitter,
                    RenderWidth = (uint)data.cameraTargetDescriptor.width, RenderHeight = (uint)data.cameraTargetDescriptor.height,
                    OutputWidth = (uint)camera.pixelWidth, OutputHeight = (uint)camera.pixelHeight,
                    Near = camera.nearClipPlane, Far = camera.farClipPlane, Fov = camera.fieldOfView * Mathf.Deg2Rad,
                    ReversedDepth = SystemInfo.usesReversedZBuffer ? 1u : 0u, DeltaTime = Time.unscaledDeltaTime
                }, PreviousToClip = vp * previous.inverse
            };
            if (!probe) { state.Frame = Time.frameCount; state.Generation = config.Generation; state.PreviousVp = vp; state.PreviousJitter = rasterJitter; }
            return packet;
        }
        private static void Probe(object __instance, RenderGraph renderGraph, ContextContainer frameData, ref TextureHandle activeCameraColorTexture)
        {
            using (var producer = Bootstrap.EnterProducer()) {
            if (!producer.Valid) return;
            var camera = frameData.Get<UniversalCameraData>();
            if (!Eligible(camera.camera)) return;
            Configuration config = Native.Configuration(camera.camera);
            if (config.Enabled == 0 || config.Backend == 6) {
                var resources = frameData.Get<UniversalResourceData>();
                if (!resources.cameraDepthTexture.IsValid() || !resources.motionVectorColor.IsValid()) return;
                var packet = Snapshot(camera, false); packet.Flags |= 34;
                Record(renderGraph, packet, activeCameraColorTexture, resources.cameraDepthTexture, resources.motionVectorColor, default);
                return;
            }
            TextureHandle output = default;
            if (!Reconstruct(__instance, renderGraph, frameData.Get<UniversalResourceData>(), camera,
                ref activeCameraColorTexture, ref output)) {
                activeCameraColorTexture = output;
                State(camera.camera).ReconstructedFrame = Time.frameCount;
            }
            }
        }
        private static void Overlay(UniversalRenderer __instance, RenderGraph renderGraph)
        {
            using (var producer = Bootstrap.EnterProducer()) {
                if (!producer.Valid || overlayFrame == Time.frameCount) return;
                var frame = (ContextContainer)frameDataProperty.GetValue(__instance);
                var camera = frame.Get<UniversalCameraData>();
                var resources = frame.Get<UniversalResourceData>();
                if (camera.camera.cameraType != CameraType.Game || camera.camera.targetTexture != null ||
                    !camera.resolveFinalTarget || !resources.backBufferColor.IsValid()) return;
                overlayFrame = Time.frameCount;
                Packet packet = Snapshot(camera, true); packet.Flags = 4;
                using (var builder = renderGraph.AddUnsafePass<PassData>("ReScaleFrame overlay", out var pass)) {
                    pass.Packet = packet;
                    builder.UseTexture(resources.backBufferColor, AccessFlags.ReadWrite);
                    builder.AllowPassCulling(false); builder.AllowGlobalStateModification(true);
                    builder.SetRenderFunc<PassData>((data, context) => {
                        using (var scope = Bootstrap.EnterProducer()) {
                            if (!scope.Valid) return;
                            Packet copy = data.Packet;
                            IntPtr owned = Native.Enqueue(ref copy);
                            if (owned != IntPtr.Zero) context.cmd.IssuePluginEventAndData(Native.Api.RenderEvent, Native.rsf_unity_get_event_id() + 1, owned);
                        }
                    });
                }
            }
        }
        private static void Hudless(RenderGraph renderGraph, ContextContainer frameData, ref TextureHandle colorBuffer)
        {
            using (var producer = Bootstrap.EnterProducer()) {
                if (!producer.Valid || hudlessFrame == Time.frameCount || !colorBuffer.IsValid()) return;
                var camera = frameData.Get<UniversalCameraData>();
                if (!Eligible(camera.camera) || !camera.resolveFinalTarget || (bool)hdrOutputProperty.GetValue(camera)) return;
                hudlessFrame = Time.frameCount;
                Packet packet = Snapshot(camera, true); packet.Flags = 10; // completed SDR colour, before the UI draw
                using (var builder = renderGraph.AddUnsafePass<PassData>("ReScaleFrame completed scene before UI", out var pass)) {
                    pass.Packet = packet; pass.Color = colorBuffer;
                    builder.UseTexture(colorBuffer, AccessFlags.Read);
                    builder.AllowPassCulling(false); builder.AllowGlobalStateModification(true);
                    builder.SetRenderFunc<PassData>(Execute);
                }
            }
        }
        private static bool Reconstruct(object __instance, RenderGraph renderGraph, UniversalResourceData resourceData,
            UniversalCameraData cameraData, ref TextureHandle source, ref TextureHandle destination)
        {
            using (var producer = Bootstrap.EnterProducer()) {
            if (!producer.Valid) return true;
            if (!Eligible(cameraData.camera)) return true;
            if (State(cameraData.camera).ReconstructedFrame == Time.frameCount) {
                destination = source;
                return false;
            }
            Configuration config = Native.Configuration(cameraData.camera);
            if (config.Enabled == 0 || config.Backend == 6 || !source.IsValid()) return true;
            try
            {
                TextureDesc descriptor = source.GetDescriptor(renderGraph);
                if (failures++ < 3) Native.Log("URP SR inputs: size=" + descriptor.sizeMode + " " + descriptor.width + "x" + descriptor.height +
                    " slices=" + descriptor.slices + " depth=" + resourceData.cameraDepthTexture.IsValid() + " motion=" + resourceData.motionVectorColor.IsValid() +
                    " before post-processing; DoF=" + VolumeManager.instance.stack.GetComponent<DepthOfField>().IsActive() +
                    " motionBlur=" + VolumeManager.instance.stack.GetComponent<MotionBlur>().IsActive());
                bool native = resourceData.cameraDepthTexture.IsValid() && resourceData.motionVectorColor.IsValid() &&
                    descriptor.slices == 1 && descriptor.dimension == TextureDimension.Tex2D;
                descriptor.sizeMode = TextureSizeMode.Explicit;
                descriptor.width = (int)config.OutputWidth; descriptor.height = (int)config.OutputHeight;
                descriptor.format = GraphicsFormat.R16G16B16A16_SFloat; descriptor.msaaSamples = MSAASamples.None;
                descriptor.enableRandomWrite = true; descriptor.useDynamicScale = false; descriptor.clearBuffer = false;
                descriptor.name = "ReScaleFrame reconstructed scene";
                TextureHandle output = renderGraph.CreateTexture(in descriptor);
                Packet packet = Snapshot(cameraData, false);
                if (!native) packet.Flags |= 16;
                updateResolution.Invoke(__instance, new object[] { renderGraph, cameraData, new Vector2Int(descriptor.width, descriptor.height) });
                Record(renderGraph, packet, source, resourceData.cameraDepthTexture, resourceData.motionVectorColor, output);
                destination = output;
                return false;
            }
            catch (Exception error)
            {
                if (++failures <= 3) Native.Log("URP reconstruction refused; original STP retained: " + error);
                return true;
            }
            }
        }
        private static void Record(RenderGraph graph, Packet packet, TextureHandle color, TextureHandle depth, TextureHandle motion, TextureHandle output)
        {
            using (var builder = graph.AddUnsafePass<PassData>("ReScaleFrame native D3D12", out var pass))
            {
                pass.Packet = packet; pass.Color = color; pass.Depth = depth; pass.Motion = motion; pass.Output = output;
                builder.UseTexture(color, AccessFlags.Read);
                if (depth.IsValid()) builder.UseTexture(depth, AccessFlags.Read);
                if (motion.IsValid()) builder.UseTexture(motion, AccessFlags.Read);
                if (output.IsValid()) builder.UseTexture(output, AccessFlags.Write);
                builder.AllowPassCulling(false); builder.AllowGlobalStateModification(true);
                builder.SetRenderFunc<PassData>(Execute);
            }
        }
        private static void Execute(PassData pass, UnsafeGraphContext context)
        {
            using (var producer = Bootstrap.EnterProducer()) {
            Packet packet = pass.Packet;
            if ((packet.Flags & 8) != 0) {
                // Imported backbuffers have an RTHandle without a RenderTexture. Resolve
                // the current engine buffer in the queued native callback, not through rt.
                if (!producer.Valid) return;
                IntPtr final = Native.Enqueue(ref packet);
                if (final != IntPtr.Zero) context.cmd.IssuePluginEventAndData(Native.Api.RenderEvent, Native.rsf_unity_get_event_id(), final);
                return;
            }
            RTHandle color = pass.Color;
            if (color == null || color.rt == null) {
                if (++failures <= 6) Native.Log("Temporal inputs refused: source has no owned RenderTexture.");
                return;
            }
            if ((packet.Flags & 2) == 0)
            {
                RTHandle output = pass.Output;
                // Queued before the event, so provider refusal always leaves a complete spatial fallback.
                context.cmd.SetRenderTarget(output);
                context.cmd.SetViewport(new Rect(0, 0, packet.Camera.OutputWidth, packet.Camera.OutputHeight));
                var validRectangle = new Vector4((float)packet.Camera.RenderWidth / color.rt.width,
                    (float)packet.Camera.RenderHeight / color.rt.height, 0, 0);
                Blitter.BlitTexture(CommandBufferHelpers.GetNativeCommandBuffer(context.cmd), color, validRectangle, 0, true);
                if (!producer.Valid) return;
                if ((packet.Flags & 16) != 0) return;
                packet.Output = output.rt.GetNativeTexturePtr();
                RTHandle depth = pass.Depth, motion = pass.Motion;
                if (depth == null || depth.rt == null || motion == null || motion.rt == null) return;
                packet.Depth = depth.rt.GetNativeTexturePtr(); packet.Motion = motion.rt.GetNativeTexturePtr();
            }
            if ((packet.Flags & 32) != 0) {
                RTHandle depth = pass.Depth, motion = pass.Motion;
                if (depth == null || depth.rt == null || motion == null || motion.rt == null) return;
                packet.Depth = depth.rt.GetNativeTexturePtr(); packet.Motion = motion.rt.GetNativeTexturePtr();
            }
            if (!producer.Valid) return;
            packet.Color = color.rt.GetNativeTexturePtr();
            IntPtr owned = Native.Enqueue(ref packet);
            if (owned != IntPtr.Zero) context.cmd.IssuePluginEventAndData(Native.Api.RenderEvent, Native.rsf_unity_get_event_id(), owned);
            }
        }
        internal static void Clear() { views.Clear(); failures = jitterReports = 0; overlayFrame = hudlessFrame = -1; }
    }
}

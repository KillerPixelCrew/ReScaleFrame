// SPDX-License-Identifier: GPL-3.0-only
#if ENABLE_UPSCALER_FRAMEWORK
using System;
using UnityEngine;
using UnityEngine.Rendering;
using UnityEngine.Rendering.RenderGraphModule;
using UnityEngine.Rendering.Universal;
using UnityEngine.Experimental.Rendering;

namespace ReScaleFrame
{
    internal sealed class ReScaleFrameFrame : ContextItem
    {
        internal Native.Packet Packet;
        public override void Reset() { Packet = default; }
    }

    public sealed class ReScaleFrameRendererFeature : ScriptableRendererFeature
    {
        private InputPass inputs;
        private FinalPass final;
        public override void Create() { inputs = new InputPass(); final = new FinalPass(); }
        public override void AddRenderPasses(ScriptableRenderer renderer, ref RenderingData data) {
            if (data.cameraData.cameraType != CameraType.Game) return;
            renderer.EnqueuePass(inputs); renderer.EnqueuePass(final);
        }
        private sealed class InputPass : ScriptableRenderPass {
            internal InputPass() {
                renderPassEvent = RenderPassEvent.BeforeRenderingPostProcessing - 1;
                ConfigureInput(ScriptableRenderPassInput.Depth | ScriptableRenderPassInput.Motion);
                requiresIntermediateTexture = true;
            }
            public override void RecordRenderGraph(RenderGraph graph, ContextContainer frameData) { }
        }
        private sealed class FinalPass : ScriptableRenderPass {
            private sealed class Data { internal TextureHandle Color, Display; internal Native.Packet Packet; }
            // This also makes URP keep post-processing output in a sampleable intermediate
            // before its final backbuffer blit and overlay UI.
            internal FinalPass() { renderPassEvent = RenderPassEvent.AfterRenderingPostProcessing; }
            public override void RecordRenderGraph(RenderGraph graph, ContextContainer frameData) {
                if (!ReScaleFrameRuntime.IsReady || !frameData.Contains<ReScaleFrameFrame>()) return;
                var frame = frameData.Get<ReScaleFrameFrame>();
                if (frame.Packet.Session == 0) return;
                var resources = frameData.Get<UniversalResourceData>();
                if (!resources.activeColorTexture.IsValid()) return;
                using (var builder = graph.AddUnsafePass<Data>("ReScaleFrame completed scene before UI", out var pass)) {
                    pass.Packet = frame.Packet; pass.Color = resources.activeColorTexture;
                    var descriptor = pass.Color.GetDescriptor(graph);
                    descriptor.sizeMode = TextureSizeMode.Explicit; descriptor.width = (int)pass.Packet.Camera.OutputWidth; descriptor.height = (int)pass.Packet.Camera.OutputHeight;
                    descriptor.format = GraphicsFormat.R8G8B8A8_UNorm; descriptor.msaaSamples = MSAASamples.None;
                    descriptor.enableRandomWrite = false; descriptor.useDynamicScale = false; descriptor.name = "ReScaleFrame display colour before UI";
                    pass.Display = graph.CreateTexture(descriptor); builder.UseTexture(pass.Display, AccessFlags.ReadWrite);
                    builder.UseTexture(pass.Color, AccessFlags.ReadWrite);
                    builder.AllowGlobalStateModification(true); builder.AllowPassCulling(false);
                    builder.SetRenderFunc<Data>((data, context) => {
                        if (!ReScaleFrameRuntime.IsReady) return;
                        RTHandle source = data.Color;
                        if (source == null) return;
                        RTHandle display = data.Display;
                        context.cmd.SetRenderTarget(display);
                        context.cmd.SetViewport(new Rect(0, 0, data.Packet.Camera.OutputWidth, data.Packet.Camera.OutputHeight));
                        Blitter.BlitTexture(CommandBufferHelpers.GetNativeCommandBuffer(context.cmd), source,
                            new Vector4(1, 1, 0, 0), ReScaleFrameUpscaler.CopyMaterial, 2);
                        source = display;
                        context.cmd.SetRenderTarget(source.nameID);
                        var packet = data.Packet; packet.Flags = 2 | 8;
                        ReScaleFrameRuntime.CompletedFrame(packet);
                        packet.Color = source.rt != null ? source.rt.GetNativeTexturePtr() : IntPtr.Zero;
                        packet.Depth = packet.Motion = packet.Output = IntPtr.Zero;
                        var queued = Native.Enqueue(ref packet);
                        if (queued != IntPtr.Zero) context.cmd.IssuePluginEventAndData(Native.RenderEvent, Native.EventId + 2, queued);
                    });
                }
            }
        }
    }
}
#endif

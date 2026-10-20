// SPDX-License-Identifier: GPL-3.0-only
using System;
using System.Collections.Generic;
using UnityEngine;
using UnityEngine.Rendering;
using UnityEngine.Rendering.RenderGraphModule;
using UnityEngine.Rendering.Universal;

namespace ReScaleFrame.Unity
{
    // Drag'n Wash's opt-in pass retains its original depth vertex/fragment programs.
    // Per-submesh history stores the resulting world positions, including GPU deformation.
    // Its private LightMode keeps it out of URP's ordinary renderer list: the graph pass
    // below supplies explicit history leases and the vertex-stage UAV for each draw.
    internal static class CharacterMotion
    {
        private const string ShaderName = "WalkNWashLit", PassName = "ReScaleFrameMotion";
        private static readonly int previousId = Shader.PropertyToID("_RSFMotionPrevious");
        private static readonly int currentId = Shader.PropertyToID("_RSFMotionCurrent");
        private static readonly int controlId = Shader.PropertyToID("_RSFMotionControl");
        private static readonly int currentVpId = Shader.PropertyToID("_RSFMotionCurrentVP");
        private static readonly int previousVpId = Shader.PropertyToID("_RSFMotionPreviousVP");
        private sealed class History : IDisposable
        {
            internal Renderer Renderer;
            internal Mesh Mesh;
            internal Material Material;
            internal GraphicsBuffer[] Buffers;
            internal int Frame = -1, Write;
            public void Dispose() { foreach (var buffer in Buffers) buffer.Dispose(); }
        }
        private struct Draw
        {
            internal Renderer Renderer;
            internal Material Material;
            internal int Submesh, Pass;
            internal GraphicsBuffer Previous, Current;
            internal Vector4 Control;
        }
        private sealed class PassData
        {
            internal Draw[] Draws;
            internal TextureHandle Color, Depth;
            internal Matrix4x4 CurrentVp, PreviousVp;
            internal int Width, Height;
        }
        private static readonly Dictionary<(int, int), History> histories = new Dictionary<(int, int), History>();
        private static readonly List<Material> materials = new List<Material>();
        private static readonly BaseRenderFunc<PassData, UnsafeGraphContext> execute = Execute;
        private static Matrix4x4 lastVp;
        private static int lastFrame = -1;
        private static int lastView;
        private static uint generation;
        private static bool reported;

        private static float Bits(uint value) => BitConverter.Int32BitsToSingle(unchecked((int)value));
        internal static Vector4 Control(uint before, uint now, uint vertices, bool valid, bool forceZero) =>
            new Vector4(Bits(before), Bits(now), Bits(vertices), Bits((valid ? 1u : 0u) | (forceZero ? 2u : 0u)));

        internal static void Record(RenderGraph renderGraph, ContextContainer frameData,
            TextureHandle motionVectorColor, TextureHandle motionVectorDepth)
        {
            using (var producer = Bootstrap.EnterProducer()) {
            if (!producer.Valid) return;
            var camera = frameData.Get<UniversalCameraData>();
            if (!UrpAdapter.Eligible(camera.camera) || !motionVectorColor.IsValid() || !motionVectorDepth.IsValid()) return;
            int frame = Time.frameCount;
            if (frame == lastFrame) return;
            var config = Native.Configuration(camera.camera);
            int view = camera.camera.GetInstanceID();
            bool consecutive = lastFrame == frame - 1 && lastView == view && generation == config.Generation;
            Matrix4x4 vp = GL.GetGPUProjectionMatrix(UrpAdapter.RawProjection(camera), true) * camera.GetViewMatrix();
            Matrix4x4 previousVp = consecutive ? lastVp : vp;
            var draws = new List<Draw>();
            foreach (var renderer in UnityEngine.Object.FindObjectsByType<Renderer>(FindObjectsSortMode.None))
            {
                if (!renderer.enabled || renderer.forceRenderingOff || renderer.shadowCastingMode == ShadowCastingMode.ShadowsOnly ||
                    renderer.isPartOfStaticBatch || renderer.motionVectorGenerationMode == MotionVectorGenerationMode.Camera ||
                    (camera.camera.cullingMask & (1 << renderer.gameObject.layer)) == 0) continue;
                Mesh mesh = null;
                if (renderer is SkinnedMeshRenderer skin) mesh = skin.sharedMesh;
                else if (renderer is MeshRenderer && renderer.TryGetComponent<MeshFilter>(out var filter)) mesh = filter.sharedMesh;
                if (mesh == null || mesh.vertexCount == 0) continue;
                renderer.GetSharedMaterials(materials);
                for (int index = 0; index < materials.Count && index < mesh.subMeshCount; ++index)
                {
                    Material material = materials[index];
                    if (material == null || material.shader.name != ShaderName) continue;
                    int pass = material.FindPass(PassName);
                    if (pass < 0) continue;
                    var key = (renderer.GetInstanceID(), index);
                    if (histories.TryGetValue(key, out var history) &&
                        (history.Renderer != renderer || history.Mesh != mesh || history.Material != material || history.Buffers[0].count != mesh.vertexCount))
                    {
                        history.Dispose(); histories.Remove(key); history = null;
                    }
                    if (history == null)
                    {
                        history = new History { Renderer = renderer, Mesh = mesh, Material = material,
                            Buffers = new[] { new GraphicsBuffer(GraphicsBuffer.Target.Structured, mesh.vertexCount, 16),
                                new GraphicsBuffer(GraphicsBuffer.Target.Structured, mesh.vertexCount, 16) } };
                        histories.Add(key, history);
                    }
                    int write = history.Write;
                    draws.Add(new Draw { Renderer = renderer, Material = material, Submesh = index, Pass = pass,
                        Current = history.Buffers[write], Previous = history.Buffers[1 - write],
                        Control = Control((uint)history.Frame + 1u, (uint)frame + 1u, (uint)mesh.vertexCount,
                            consecutive && history.Frame == frame - 1,
                            renderer.motionVectorGenerationMode == MotionVectorGenerationMode.ForceNoMotion) });
                    history.Write = 1 - write; history.Frame = frame;
                }
            }
            var expired = new List<(int, int)>();
            foreach (var item in histories)
                if (item.Value.Renderer == null || item.Value.Frame < frame - 2) expired.Add(item.Key);
            foreach (var key in expired) { histories[key].Dispose(); histories.Remove(key); }
            lastFrame = frame; lastView = view; lastVp = vp; generation = config.Generation;
            if (draws.Count == 0) return;
            using (var builder = renderGraph.AddUnsafePass<PassData>("ReScaleFrame character motion", out var passData))
            {
                passData.Draws = draws.ToArray(); passData.Color = motionVectorColor; passData.Depth = motionVectorDepth;
                passData.CurrentVp = vp; passData.PreviousVp = previousVp;
                passData.Width = camera.cameraTargetDescriptor.width; passData.Height = camera.cameraTargetDescriptor.height;
                builder.UseTexture(motionVectorColor, AccessFlags.ReadWrite);
                builder.UseTexture(motionVectorDepth, AccessFlags.ReadWrite);
                foreach (var draw in passData.Draws)
                {
                    var before = renderGraph.ImportBuffer(draw.Previous);
                    var now = renderGraph.ImportBuffer(draw.Current);
                    builder.UseBuffer(before, AccessFlags.Read); builder.UseBuffer(now, AccessFlags.Write);
                }
                builder.AllowPassCulling(false); builder.AllowGlobalStateModification(true);
                builder.SetRenderFunc(execute);
            }
            if (!reported) { reported = true; Native.Log("Character deformation motion pass recorded: " + draws.Count + " submeshes."); }
            }
        }

        private static void Execute(PassData data, UnsafeGraphContext context)
        {
            using (var producer = Bootstrap.EnterProducer()) {
            if (!producer.Valid) return;
            RTHandle color = data.Color, depth = data.Depth;
            var cmd = CommandBufferHelpers.GetNativeCommandBuffer(context.cmd);
            cmd.SetRenderTarget(color.nameID, depth.nameID);
            cmd.SetViewport(new Rect(0, 0, data.Width, data.Height));
            cmd.SetGlobalMatrix(currentVpId, data.CurrentVp); cmd.SetGlobalMatrix(previousVpId, data.PreviousVp);
            foreach (var draw in data.Draws)
            {
                if (draw.Renderer == null || draw.Material == null) continue;
                cmd.SetGlobalVector(controlId, draw.Control); cmd.SetGlobalBuffer(previousId, draw.Previous);
                cmd.SetGlobalBuffer(currentId, draw.Current);
                cmd.SetRandomWriteTarget(1, draw.Current, true);
                cmd.DrawRenderer(draw.Renderer, draw.Material, draw.Submesh, draw.Pass);
                cmd.ClearRandomWriteTargets();
            }
            }
        }

        internal static void Clear()
        {
            foreach (var history in histories.Values) history.Dispose();
            histories.Clear(); materials.Clear(); lastFrame = -1; lastView = 0; generation = 0; reported = false;
        }
    }
}

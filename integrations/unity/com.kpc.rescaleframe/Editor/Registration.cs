// SPDX-License-Identifier: GPL-3.0-only
#if ENABLE_UPSCALER_FRAMEWORK
using UnityEditor;
using UnityEngine.Rendering;
using UnityEngine.Rendering.Universal;
using UnityEngine;

namespace ReScaleFrame.Editor
{
    internal static class Registration
    {
        [InitializeOnLoadMethod]
        private static void Register() {
            UpscalerRegistry.Register<ReScaleFrameUpscaler, ReScaleFrameOptions>(ReScaleFrameUpscaler.Name);
            string package = UnityEditor.PackageManager.PackageInfo.FindForAssembly(typeof(Registration).Assembly).resolvedPath;
            ReScaleFrameRuntime.RuntimeDirectory = System.IO.Path.Combine(package, "Runtime~");
            ReScaleFrameRuntime.ProjectId = BuildProcessor.ProjectId;
            EditorApplication.playModeStateChanged -= PlayModeChanged;
            EditorApplication.playModeStateChanged += PlayModeChanged;
            AssemblyReloadEvents.beforeAssemblyReload -= ReScaleFrameRuntime.Stop;
            AssemblyReloadEvents.beforeAssemblyReload += ReScaleFrameRuntime.Stop;
        }
        private static void PlayModeChanged(PlayModeStateChange state) {
            if (state == PlayModeStateChange.ExitingPlayMode) ReScaleFrameRuntime.Stop();
        }

        [MenuItem("Tools/ReScaleFrame/Set Up Active URP Asset")]
        public static void SetUpActivePipeline() {
            var asset = GraphicsSettings.currentRenderPipeline as UniversalRenderPipelineAsset;
            if (asset == null) throw new System.InvalidOperationException("Select a URP pipeline asset first.");
            Undo.RecordObject(asset, "Set up ReScaleFrame");
            var options = asset.GetUpscalerOptions(ReScaleFrameUpscaler.Name) as ReScaleFrameOptions;
            if (options == null) {
                options = ScriptableObject.CreateInstance<ReScaleFrameOptions>(); options.name = "ReScaleFrame Options";
                AssetDatabase.AddObjectToAsset(options, asset); asset.upscalerOptions.Add(options);
            }
            asset.upscalerName = ReScaleFrameUpscaler.Name;
            var serialized = new SerializedObject(asset);
            var renderers = serialized.FindProperty("m_RendererDataList");
            for (int i = 0; i < renderers.arraySize; i++) {
                var renderer = renderers.GetArrayElementAtIndex(i).objectReferenceValue as ScriptableRendererData;
                if (renderer == null || renderer.rendererFeatures.Exists(item => item is ReScaleFrameRendererFeature)) continue;
                Undo.RecordObject(renderer, "Add ReScaleFrame rendering");
                var feature = ScriptableObject.CreateInstance<ReScaleFrameRendererFeature>(); feature.name = "ReScaleFrame";
                AssetDatabase.AddObjectToAsset(feature, renderer); renderer.rendererFeatures.Add(feature);
                EditorUtility.SetDirty(renderer);
            }
            EditorUtility.SetDirty(asset); AssetDatabase.SaveAssets();
            Selection.activeObject = options;
        }
    }
}
#endif

// SPDX-License-Identifier: GPL-3.0-only
#if ENABLE_UPSCALER_FRAMEWORK
using System;
using System.IO;
using UnityEditor;
using UnityEditor.Build;
using UnityEditor.SceneManagement;
using UnityEngine;
using UnityEngine.Rendering;
using UnityEngine.Rendering.Universal;
using UnityEngine.SceneManagement;

namespace ReScaleFrame.Editor
{
    public static class ValidationScene
    {
        private const string DirectoryPath = "Assets/ReScaleFrameValidation";
        public const string ScenePath = DirectoryPath + "/TemporalDemo.unity";
        [MenuItem("Tools/ReScaleFrame/Import Temporal Demo")]
        public static void ImportDemo() {
            string package = UnityEditor.PackageManager.PackageInfo.FindForAssembly(typeof(ValidationScene).Assembly).resolvedPath;
            Directory.CreateDirectory(DirectoryPath);
            foreach (string file in Directory.GetFiles(Path.Combine(package, "Samples~", "TemporalDemo")))
                File.Copy(file, Path.Combine(DirectoryPath, Path.GetFileName(file)), true);
            AssetDatabase.Refresh();
        }
        [MenuItem("Tools/ReScaleFrame/Create Temporal Validation Scene")]
        public static void CreateScene() {
            Type demoType = null;
            foreach (var type in TypeCache.GetTypesDerivedFrom<MonoBehaviour>())
                if (type.FullName == "ReScaleFrame.Demo.TemporalDemo") { demoType = type; break; }
            if (demoType == null) throw new InvalidOperationException("Import the temporal demo and let it compile first.");
            if (File.Exists(ScenePath)) throw new InvalidOperationException("The validation scene already exists; open it instead of replacing it.");
            Registration.SetUpActivePipeline();
            var old = SceneManager.GetActiveScene();
            var scene = EditorSceneManager.NewScene(NewSceneSetup.EmptyScene, NewSceneMode.Additive); SceneManager.SetActiveScene(scene);
            RenderSettings.ambientLight = new Color(0.3f, 0.35f, 0.45f);
            var cameraObject = new GameObject("Main Camera", typeof(Camera)); cameraObject.tag = "MainCamera";
            var camera = cameraObject.GetComponent<Camera>(); camera.transform.position = new Vector3(0, 2.3f, -7);
            camera.transform.LookAt(new Vector3(0, 0.6f, 0)); camera.nearClipPlane = 0.1f; camera.farClipPlane = 200;
            camera.allowHDR = true; camera.GetUniversalAdditionalCameraData().renderPostProcessing = true;
            var light = new GameObject("Sun", typeof(Light)).GetComponent<Light>(); light.type = LightType.Directional;
            light.intensity = 2; light.transform.rotation = Quaternion.Euler(40, -30, 0);
            var material = new Material(Shader.Find("Universal Render Pipeline/Lit")); material.color = new Color(0.75f, 0.24f, 0.1f);
            AssetDatabase.CreateAsset(material, DirectoryPath + "/MovingMaterial.mat");
            var floor = GameObject.CreatePrimitive(PrimitiveType.Plane); floor.name = "Floor"; floor.transform.localScale = new Vector3(2, 1, 2);
            var cube = GameObject.CreatePrimitive(PrimitiveType.Cube); cube.name = "Moving Cube"; cube.transform.position = new Vector3(0, 0.7f, 0);
            cube.GetComponent<Renderer>().sharedMaterial = material; cube.GetComponent<Renderer>().motionVectorGenerationMode = MotionVectorGenerationMode.Object;
            var clip = new AnimationClip { legacy = true, wrapMode = WrapMode.Loop, name = "Independent object motion" };
            clip.SetCurve("", typeof(Transform), "localPosition.x", AnimationCurve.EaseInOut(0, -2, 2, 2));
            AssetDatabase.CreateAsset(clip, DirectoryPath + "/Motion.anim");
            var animation = cube.AddComponent<Animation>(); animation.AddClip(clip, "Motion"); animation.clip = clip; animation.playAutomatically = true;
            for (int i = 0; i < 10; i++) {
                var sphere = GameObject.CreatePrimitive(PrimitiveType.Sphere); sphere.name = "Static detail " + i;
                sphere.transform.position = new Vector3((i - 5) * 0.7f, 0.3f, 2 + (i % 2)); sphere.transform.localScale = Vector3.one * 0.6f;
            }
            new GameObject("ReScaleFrame Demo").AddComponent(demoType);
            EditorSceneManager.SaveScene(scene, ScenePath); EditorSceneManager.CloseScene(scene, true);
            SceneManager.SetActiveScene(old); AssetDatabase.SaveAssets();
        }
        public static void BuildPlayer(string path) {
            var report = BuildPipeline.BuildPlayer(new BuildPlayerOptions {
                scenes = new[] { ScenePath }, locationPathName = path, target = BuildTarget.StandaloneWindows64,
                options = BuildOptions.Development
            });
            if (report.summary.result != UnityEditor.Build.Reporting.BuildResult.Succeeded) throw new BuildFailedException("Temporal demo player build failed: " + report.summary.result);
        }
    }
}
#endif

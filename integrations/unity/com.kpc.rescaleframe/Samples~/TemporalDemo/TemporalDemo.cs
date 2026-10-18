// SPDX-License-Identifier: GPL-3.0-only
using System;
using System.Collections;
using System.IO;
using UnityEngine;
using UnityEngine.Rendering.Universal;

namespace ReScaleFrame.Demo
{
    public sealed class TemporalDemo : MonoBehaviour
    {
        [Serializable] private sealed class Report {
            public string api, requestedSR, activeSR, fgProvider, latency;
            public int srResult, fgResult, renderWidth, renderHeight, outputWidth, outputHeight, generated, maximum;
            public bool fgAvailable, fgActive, pclAvailable, latencyReport;
            public ulong evaluated, refused, tagged, sleep, markers;
            public uint deferred;
        }
        private ReScaleFrameOptions options;
        private float nextReport, started;
        private float quitAfter;
        private string output;
        private bool captured, engineBaseline;
        private Vector2 scroll;
        private static readonly string[] srNames = { "Off", "DLSS", "FSR 2", "FSR 3", "FSR 4", "XeSS", "FSR 1", "Auto" };
        private static readonly string[] qualityNames = { "Native", "Quality", "Balanced", "Performance", "Ultra Performance", "Ultra Quality" };
        private static readonly FrameGeneration[] fgValues = { FrameGeneration.Auto, FrameGeneration.DLSS, FrameGeneration.FSR3, FrameGeneration.FSR4, FrameGeneration.XeSS };
        private static readonly string[] fgNames = { "Auto", "DLSS", "FSR 3", "FSR 4", "XeSS" };
        private static readonly DlssModel[] modelValues = { DlssModel.Default, DlssModel.E, DlssModel.F, DlssModel.J, DlssModel.K, DlssModel.L, DlssModel.M };
        private static readonly string[] modelNames = { "Default", "E", "F", "J", "K", "L", "M" };
        private static readonly string[] latencyNames = { "Off", "On", "On + Boost" };
        private void Start() {
            options = UniversalRenderPipeline.asset.GetUpscalerOptions(ReScaleFrameUpscaler.Name) as ReScaleFrameOptions;
            if (options == null) { Debug.LogError("ReScaleFrame demo requires the URP setup menu first."); enabled = false; return; }
            foreach (string arg in Environment.GetCommandLineArgs()) {
                if (arg.StartsWith("--rsf-sr=")) options.superResolution = (SuperResolution)Enum.Parse(typeof(SuperResolution), arg.Substring(9), true);
                if (arg.StartsWith("--rsf-fg=")) options.frameGeneration = (FrameGeneration)Enum.Parse(typeof(FrameGeneration), arg.Substring(9), true);
                if (arg.StartsWith("--rsf-generate=")) options.generateFrames = bool.Parse(arg.Substring(15));
                if (arg.StartsWith("--rsf-latency=")) options.latency = (LatencyMode)Enum.Parse(typeof(LatencyMode), arg.Substring(14), true);
                if (arg.StartsWith("--rsf-output=")) output = arg.Substring(13);
                if (arg.StartsWith("--rsf-quit-after=")) quitAfter = float.Parse(arg.Substring(17), System.Globalization.CultureInfo.InvariantCulture);
                if (arg == "--rsf-engine-baseline") engineBaseline = true;
            }
            if (!string.IsNullOrEmpty(output)) Directory.CreateDirectory(output);
            started = Time.realtimeSinceStartup;
            if (engineBaseline) {
                UniversalRenderPipeline.asset.upscalerName = "Bilinear";
                foreach (var feature in Resources.FindObjectsOfTypeAll<ReScaleFrameRendererFeature>()) feature.SetActive(false);
                ReScaleFrameRuntime.Stop(); Debug.Log("RSF_VALIDATION Unity URP engine baseline");
            } else ReScaleFrameRuntime.Apply(options);
        }
        private void Update() {
            if (options == null) return;
            if (!engineBaseline) ReScaleFrameRuntime.Apply(options);
            float elapsed = Time.realtimeSinceStartup - started;
            if (!captured && elapsed > 8 && !string.IsNullOrEmpty(output)) { captured = true; StartCoroutine(Capture()); }
            if (elapsed >= nextReport) {
                nextReport = elapsed + 1;
                if (ReScaleFrameRuntime.TryGetStatus(out var s)) {
                    var report = new Report {
                        api = SystemInfo.graphicsDeviceType.ToString(), requestedSR = s.RequestedSR.ToString(), activeSR = s.ActiveSR.ToString(),
                        srResult = s.ReconstructionResult, fgResult = s.FrameGenerationResult, fgProvider = s.ActiveGenerationProvider.ToString(),
                        fgAvailable = s.FrameGenerationAvailable, fgActive = s.FrameGenerationActive, generated = s.EffectiveGeneratedFrames, maximum = s.MaxGeneratedFrames,
                        latency = s.EffectiveLatency.ToString(), pclAvailable = s.PclAvailable, latencyReport = s.LatencyReportAvailable,
                        renderWidth = s.RenderWidth, renderHeight = s.RenderHeight, outputWidth = s.OutputWidth, outputHeight = s.OutputHeight,
                        evaluated = s.EvaluatedFrames, refused = s.RefusedFrames, tagged = s.TaggedFrames,
                        sleep = s.SleepCalls, markers = s.MarkerCalls, deferred = s.DeferredReason
                    };
                    string json = JsonUtility.ToJson(report); Debug.Log("RSF_VALIDATION " + json);
                    if (!string.IsNullOrEmpty(output)) File.AppendAllText(Path.Combine(output, "frames.jsonl"), json + "\n");
                } else if (!engineBaseline) Debug.Log("RSF_VALIDATION waiting: " + ReScaleFrameRuntime.LastError);
            }
            if (quitAfter > 0 && elapsed >= quitAfter) Application.Quit();
        }
        private IEnumerator Capture() { yield return new WaitForEndOfFrame(); ScreenCapture.CaptureScreenshot(Path.Combine(output, "frame.png")); }
        private void OnGUI() {
            if (options == null) return;
            if (engineBaseline) { GUI.Label(new Rect(15, 15, 400, 30), "Unity URP engine baseline"); return; }
            bool haveStatus = ReScaleFrameRuntime.TryGetStatus(out var status);
            GUILayout.BeginArea(new Rect(15, 15, Mathf.Min(500, Screen.width - 30), Mathf.Min(640, Screen.height - 30)), GUI.skin.box);
            scroll = GUILayout.BeginScrollView(scroll);
            GUILayout.Label("ReScaleFrame URP");
            if (ReScaleFrameRuntime.TryGetFrameRates(out var fps)) {
                GUILayout.Label("Base: " + fps.BaseFPS.ToString("F1") + " FPS    FG/output: " +
                    (fps.HasOutputFPS ? fps.OutputFPS.ToString("F1") + " FPS" : "awaiting presented-frame counter"));
            }
            GUILayout.Label("Super resolution");
            options.superResolution = (SuperResolution)GUILayout.SelectionGrid((int)options.superResolution, srNames, 4);
            GUILayout.Label("Quality level");
            bool dlssQuality = options.superResolution == SuperResolution.DLSS ||
                (options.superResolution == SuperResolution.Auto && SystemInfo.graphicsDeviceVendorID == 0x10de);
            for (int row = 0; row < 2; row++) {
                GUILayout.BeginHorizontal();
                for (int column = 0; column < 3; column++) {
                    int quality = row * 3 + column;
                    bool enabled = GUI.enabled; GUI.enabled = enabled && !(dlssQuality && quality == (int)ReconstructionQuality.UltraQuality);
                    bool selected = (int)options.quality == quality;
                    if (GUILayout.Toggle(selected, qualityNames[quality], GUI.skin.button) && !selected) options.quality = (ReconstructionQuality)quality;
                    GUI.enabled = enabled;
                }
                GUILayout.EndHorizontal();
            }
            GUILayout.Label("DLSS model request");
            int selectedModel = Array.IndexOf(modelValues, options.dlssModel);
            int model = GUILayout.SelectionGrid(selectedModel, modelNames, 7);
            if (model >= 0) options.dlssModel = modelValues[model];
            GUILayout.Label("Driver overrides can change the effective DLSS model.");
            options.generateFrames = GUILayout.Toggle(options.generateFrames, "Frame generation");
            int selectedProvider = Array.IndexOf(fgValues, options.frameGeneration);
            int provider = GUILayout.SelectionGrid(selectedProvider, fgNames, 5);
            if (provider >= 0) options.frameGeneration = fgValues[provider];
            GUILayout.Label("Requested MFG: " + (options.generatedFrames + 1) + "x (" + options.generatedFrames + " generated per rendered frame)");
            int maximum = haveStatus ? Mathf.Clamp(status.MaxGeneratedFrames, 0, 15) : 0;
            if (maximum > 0) {
                GUILayout.BeginHorizontal();
                for (int generated = 1; generated <= maximum; generated++) {
                    if (generated > 1 && (generated - 1) % 6 == 0) { GUILayout.EndHorizontal(); GUILayout.BeginHorizontal(); }
                    bool selected = options.generatedFrames == generated;
                    if (GUILayout.Toggle(selected, (generated + 1) + "x", GUI.skin.button) && !selected) options.generatedFrames = generated;
                }
                GUILayout.EndHorizontal();
                int effective = status.FrameGenerationActive ? status.EffectiveGeneratedFrames + 1 : 1;
                GUILayout.Label("Effective: " + effective + "x; SDK maximum: " + (maximum + 1) + "x");
            } else GUILayout.Label("MFG capacity: awaiting the selected provider's SDK response.");
            GUILayout.Label("Low latency");
            options.latency = (LatencyMode)GUILayout.SelectionGrid((int)options.latency, latencyNames, 3);
            if (haveStatus) {
                GUILayout.Label("Active SR: " + status.ActiveSR + " (" + status.RenderWidth + " × " + status.RenderHeight + " → " + status.OutputWidth + " × " + status.OutputHeight + ")");
                GUILayout.Label("Frame generation: " + (status.FrameGenerationActive ? "Active" : "Inactive") + ", maximum " + status.MaxGeneratedFrames + " additional frames");
                GUILayout.Label("Latency: " + status.EffectiveLatency + "; PCL " + (status.PclAvailable ? "available" : "unavailable"));
            } else GUILayout.Label(ReScaleFrameRuntime.LastError ?? "Preparing rendering");
            GUILayout.EndScrollView();
            GUILayout.EndArea();
        }
    }
}

// SPDX-License-Identifier: GPL-3.0-only
#if ENABLE_UPSCALER_FRAMEWORK
using UnityEngine;
using UnityEngine.Rendering;

namespace ReScaleFrame
{
    public enum SuperResolution { Off, DLSS, FSR2, FSR3, FSR4, XeSS, FSR1, Auto }
    public enum ReconstructionQuality { Native, Quality, Balanced, Performance, UltraPerformance, UltraQuality }
    public enum FrameGeneration { DLSS = 1, FSR3 = 3, FSR4 = 4, XeSS = 5, Auto = 7 }
    public enum LatencyMode { Off, On, OnWithBoost }
    public enum DlssModel { Default = 0, E = 5, F = 6, J = 10, K = 11, L = 12, M = 13 }

    [CreateAssetMenu(menuName = "Rendering/ReScaleFrame Options")]
    public sealed class ReScaleFrameOptions : UpscalerOptions
    {
        public SuperResolution superResolution = SuperResolution.Auto;
        public ReconstructionQuality quality = ReconstructionQuality.Quality;
        [Tooltip("Requested DLSS model. Driver overrides can change the effective model.")]
        public DlssModel dlssModel;
        [Range(0, 1)] public float fsr1Sharpness = 0.2f;
        public FrameGeneration frameGeneration = FrameGeneration.Auto;
        public bool generateFrames;
        [Range(1, 15)] public int generatedFrames = 1;
        public LatencyMode latency = LatencyMode.On;
        [Min(0)] public int frameLimit;
        [Tooltip("Disable during menus, loading or paused scenes that should not interpolate.")]
        public bool gameplay = true;

        internal Native.Settings Snapshot => new Native.Settings {
            Size = Native.SettingsSize, Version = 1, Sr = (uint)superResolution, Quality = (uint)quality,
            Preset = (uint)dlssModel, Fg = (uint)frameGeneration, FgMode = generateFrames ? 1u : 0u,
            Generated = (uint)Mathf.Clamp(generatedFrames, 1, 15), Latency = (uint)latency,
            LimitUs = frameLimit > 0 ? (uint)(1000000 / frameLimit) : 0,
            Gameplay = gameplay && Time.timeScale > 0 && Application.isFocused ? 1u : 0u
        };

        private void OnEnable() { upscalerName = ReScaleFrameUpscaler.Name; }
    }
}
#endif

// SPDX-License-Identifier: GPL-3.0-only
#if ENABLE_UPSCALER_FRAMEWORK
using UnityEngine;
using UnityEngine.Rendering;

namespace ReScaleFrame
{
    public enum SharpeningMode { Off, RCAS, DepthRCAS, DepthDAS, VendorFSROnly }
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
        [Tooltip("Engine-owned spatial FSR1 uses its own RCAS; native temporal sharpening below does not apply to FSR1.")]
        [Range(0, 1)] public float fsr1Sharpness = 0.2f;
        [Header("Native temporal post-upscale sharpening")]
        public SharpeningMode sharpening_mode;
        [Range(0.0f, 1.0f)] public float sharpening_sharpness = 0.4f;
        public bool sharpening_mas;
        [Range(-1.0f, 1.0f)] public float sharpening_motion_sharpness = 0.2f;
        [Range(0.0f, 100.0f)] public float sharpening_motion_threshold = 0.0f;
        [Range(0.01f, 100.0f)] public float sharpening_motion_scale_limit = 10.0f;
        public bool sharpening_contrast_enabled;
        [Range(0.0f, 2.0f)] public float sharpening_contrast = 0.0f;
        [Range(0.0f, 600.0f)] public float sharpening_depth_scale = 35.0f;
        [Range(0.0f, 1.0f)] public float sharpening_depth_bias = 0.001f;
        public bool sharpening_clamp_output;
        public bool sharpening_debug;
        public FrameGeneration frameGeneration = FrameGeneration.Auto;
        public bool generateFrames;
        [Range(1, 15)] public int generatedFrames = 1;
        public LatencyMode latency = LatencyMode.On;
        [Min(0)] public int frameLimit;
        [Tooltip("Disable during menus, loading or paused scenes that should not interpolate.")]
        public bool gameplay = true;

        internal Native.Settings Snapshot => new Native.Settings {
            Size = Native.SettingsSize, Version = 2, Sr = (uint)superResolution, Quality = (uint)quality,
            Preset = (uint)dlssModel, Fg = (uint)frameGeneration, FgMode = generateFrames ? 1u : 0u,
            Generated = (uint)Mathf.Clamp(generatedFrames, 1, 15), Latency = (uint)latency,
            LimitUs = frameLimit > 0 ? (uint)(1000000 / frameLimit) : 0,
            Sharpen = new Native.Sharpening { Size = 56, Version = 1,
                mode =  (uint)sharpening_mode,
                sharpness = Mathf.Clamp(sharpening_sharpness, 0.0f, 1.0f),
                mas = sharpening_mas ? 1u : 0u,
                motion_sharpness = Mathf.Clamp(sharpening_motion_sharpness, -1.0f, 1.0f),
                motion_threshold = Mathf.Clamp(sharpening_motion_threshold, 0.0f, 100.0f),
                motion_scale_limit = Mathf.Clamp(sharpening_motion_scale_limit, 0.01f, 100.0f),
                contrast_enabled = sharpening_contrast_enabled ? 1u : 0u,
                contrast = Mathf.Clamp(sharpening_contrast, 0.0f, 2.0f),
                depth_scale = Mathf.Clamp(sharpening_depth_scale, 0.0f, 600.0f),
                depth_bias = Mathf.Clamp(sharpening_depth_bias, 0.0f, 1.0f),
                clamp_output = sharpening_clamp_output ? 1u : 0u,
                debug = sharpening_debug ? 1u : 0u },
            Gameplay = gameplay && Time.timeScale > 0 && Application.isFocused ? 1u : 0u
        };

        private void OnEnable() { upscalerName = ReScaleFrameUpscaler.Name; }
    }
}
#endif

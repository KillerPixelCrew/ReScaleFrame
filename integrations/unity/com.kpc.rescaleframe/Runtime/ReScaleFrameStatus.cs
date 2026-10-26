// SPDX-License-Identifier: GPL-3.0-only
#if ENABLE_UPSCALER_FRAMEWORK
namespace ReScaleFrame
{
    public readonly struct ReScaleFrameStatus
    {
        public SuperResolution RequestedSR { get; }
        public SuperResolution ActiveSR { get; }
        public int RenderWidth { get; }
        public int RenderHeight { get; }
        public int OutputWidth { get; }
        public int OutputHeight { get; }
        public int ReconstructionResult { get; }
        public int FrameGenerationResult { get; }
        public bool FrameGenerationAvailable { get; }
        public bool FrameGenerationActive { get; }
        public FrameGeneration ActiveGenerationProvider { get; }
        public int MaxGeneratedFrames { get; }
        public int EffectiveGeneratedFrames { get; }
        public bool LowLatencyAvailable { get; }
        public LatencyMode EffectiveLatency { get; }
        public uint DeferredReason { get; }
        public ulong EvaluatedFrames { get; }
        public ulong RefusedFrames { get; }
        public ulong TaggedFrames { get; }
        public ulong SleepCalls { get; }
        public ulong MarkerCalls { get; }
        public bool PclAvailable { get; }
        public bool LatencyReportAvailable { get; }
        internal ReScaleFrameStatus(Native.Status value) {
            RequestedSR = (SuperResolution)value.RequestedSR; ActiveSR = (SuperResolution)value.ActiveSR;
            RenderWidth = (int)value.RenderWidth; RenderHeight = (int)value.RenderHeight;
            OutputWidth = (int)value.OutputWidth; OutputHeight = (int)value.OutputHeight;
            ReconstructionResult = value.SrResult; FrameGenerationResult = value.FgResult;
            FrameGenerationAvailable = value.FgAvailable != 0; FrameGenerationActive = value.FgActive != 0;
            ActiveGenerationProvider = (FrameGeneration)value.FgBackend; MaxGeneratedFrames = (int)value.MaxGeneratedFrames;
            EffectiveGeneratedFrames = (int)value.GeneratedFrames; LowLatencyAvailable = value.LatencyAvailable != 0;
            EffectiveLatency = (LatencyMode)value.EffectiveLatency; DeferredReason = value.Reason;
            EvaluatedFrames = value.EvaluatedFrames; RefusedFrames = value.RefusedFrames; TaggedFrames = value.TaggedFrames;
            SleepCalls = value.SleepCalls; MarkerCalls = value.MarkerCalls;
            PclAvailable = value.PclAvailable != 0; LatencyReportAvailable = value.LatencyReportAvailable != 0;
        }
    }
}
#endif

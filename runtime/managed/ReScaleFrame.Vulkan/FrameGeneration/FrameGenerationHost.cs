using Silk.NET.Vulkan;
using VulkanStory.Contracts;

namespace ReScaleFrame.Vulkan;

/// <summary>Borrowed renderer IDs and dimensions for one current-frame target.</summary>
internal readonly record struct FrameGenerationTarget(int Width, int Height, int DepthTextureId,
    int Color, int ColorCount)
{
    internal bool HasColor => ColorCount > 0;
}

/// <summary>Current world/view identity and temporal facts, without game objects.</summary>
internal readonly record struct FrameGenerationFrame(ulong FrameId, bool CanGenerate,
    bool MotionValid, bool HasCamera, TemporalProviderFrame Provider,
    float CameraDeltaX, float CameraDeltaY, float CameraDeltaZ);

internal interface IFrameGenerationSettings
{
    string Provider { get; }
    int Multiplier { get; }
}

/// <summary>Scene input production borrowed only for the synchronous Generate call.</summary>
internal interface IFrameGenerationInputs
{
    int MotionAttachment { get; }
    FrameGenerationTarget Primary { get; }
    FrameGenerationTarget Output { get; }
    FrameGenerationTarget Scene { get; }
    FrameGenerationTarget Ui { get; }
    bool TryGetCamera(int width, int height, out NgxFrameGenerationCamera camera);
    bool TryPrepareMotion(out int texture, out string reason);
    bool ComposeGeneratedFrameUi();
    void FillXessCamera(ref XessPresentationFrame constants, double originX, double originY, double originZ);
}

/// <summary>Renderer resource, command-recording and presentation operations used by FG.</summary>
internal interface IFrameGenerationDevice : IUpscalerDevice
{
    ulong LatencyFrameId { get; }
    bool StreamlineFrameGenerationAvailable { get; }
    bool StreamlineFrameGenerationReady { get; }
    bool Fsr3ProxyReady { get; }
    bool Fsr3DirectPresentation { get; }
    nint Fsr3ProxyContext { get; }
    Format Fsr3ProxyFormat { get; }
    string? Fsr3ProxyFailure { get; }
    string? XessProxyFailure { get; }
    bool CanPresentGeneratedFrame { get; }
    bool PresentationVsyncEnabled { get; }
    int FrameGenerationMultiplier { get; set; }
    uint XessConfiguredGeneratedFrames { get; }
    uint? XessLastReportedGeneratedLimit { get; }
    void UpscalerHandles(out nint instance, out nint physical, out nint logical);
    void SetFrameGenerationPresentation(string provider);
    void ResetGeneratedFramePresent();
    bool QueueGeneratedFrameForPresent(int texture);
    void SuspendStreamlineFrameGeneration();
    void PauseXessFrameGeneration();
    void SuspendXessFrameGeneration();
    int TakeStreamlinePresentError();
    int SetStreamlineFrameGeneration(bool enabled, uint generatedFrames = 1);
    int GetStreamlineFrameGenerationStateDetails(out StreamlineFrameGenerationState state);
    int TagStreamlineFrame(int depth, int motion, int hudless, int ui,
        int uprightDepth, int uprightMotion, int uprightHudless, int uprightUi,
        in NgxFrameGenerationCamera camera, bool reset);
    int PrepareXessFrame(int depth, int motion, int motionRg, int hudless, int ui,
        in XessPresentationFrame constants);
    int EvaluateFsr3FrameGeneration(Fsr3Native api, nint context,
        int backbuffer, int depth, int motion, int motionRg, int hudless,
        int ui, int uprightScene, int uprightUi, int uprightDepth,
        int uprightMotion, int uprightOutput, int output,
        in TemporalProviderFrame temporal, ulong frameId);
    void NoteSdkActualPresents(uint count);
}

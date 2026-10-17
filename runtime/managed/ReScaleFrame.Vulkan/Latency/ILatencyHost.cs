namespace ReScaleFrame.Vulkan;

internal interface IXessLatencyPresenter
{
    XessFgRuntime Runtime { get; }
    void WaitForGpuIdle();
}

/// <summary>Current presentation resources and input-frame identity borrowed by the shared latency owner.</summary>
internal interface ILatencyHost
{
    StreamlineRuntime? Streamline { get; }
    IXessLatencyPresenter? XessPresenter { get; }
    bool XessSelected { get; }
    bool XessGenerationEnabled { get; }
    bool XessGenerationPaused { get; }
    bool HasSwapchain { get; }
    ulong LatencyFrameId { get; }
    ulong RealPresentId { get; }
    void ReserveFramePresentIds(bool mayGenerate);
    void SetXessGenerationEnabled(bool enabled);
    void ReportValidation(string message);
    long BeginReflexWait();
    void EndReflexWait(long start);
}

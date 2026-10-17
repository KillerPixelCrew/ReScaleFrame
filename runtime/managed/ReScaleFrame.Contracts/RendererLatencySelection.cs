namespace VulkanStory.Contracts;

/// <summary>Immutable host selection read by the retained latency path.</summary>
/// <param name="RequestedMode">Stored mode token: off, on, or boost.</param>
/// <param name="EffectiveFrameGeneration">Effective FG token used to enforce low latency during FG.</param>
public sealed record RendererLatencySelection(string RequestedMode, string EffectiveFrameGeneration)
{
    // Preserve the existing product rule: active FG requires low latency even
    // when the user's stored selection is Off. Native SDK calls still gate on availability.
    /// <summary>SDK mode number: 0 for Off, 1 for On, 2 for Boost; active FG keeps at least On.</summary>
    public int EffectiveMode => RequestedMode switch
    {
        "off" when EffectiveFrameGeneration is not ("dlss" or "fsr3" or "xess") => 0,
        "boost" => 2,
        _ => 1,
    };
}

/// <summary>Receives the host's render-start boundary without introducing game types into latency code.</summary>
public interface ILatencyStageListener
{
    /// <summary>Marks the beginning of rendering for the host's current real frame.</summary>
    void OnFrameRenderStart();
}

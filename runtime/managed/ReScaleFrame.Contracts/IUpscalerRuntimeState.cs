namespace VulkanStory.Contracts;

/// <summary>
/// The game/mod-side upscaler selection and session status. Providers receive
/// this interface instead of reading injected game configuration statics.
/// </summary>
public interface IUpscalerRuntimeState
{
    /// <summary>Requested reconstruction provider before session refusals or stereo gating.</summary>
    string RequestedUpscaler => DlssRequested ? "dlss" : "off";
    /// <summary>Provider permitted by the host's current world/view state.</summary>
    string EffectiveUpscaler => RequestedUpscaler;
    /// <summary>Whether the host retained a refusal for this provider in the current session.</summary>
    bool IsDisabled(string provider) => false;
    /// <summary>Whether the host currently requests DLSS reconstruction.</summary>
    bool DlssRequested { get; }
    /// <summary>Requested quality token interpreted by the selected provider.</summary>
    string Quality { get; }
    /// <summary>Requested DLSS model preset hint; default follows the installed runtime.</summary>
    string DlssPreset => "default";
    /// <summary>Host-supplied mip adjustment used when deriving the active plan.</summary>
    float LodBiasOffset { get; }
    /// <summary>Publishes the effective reconstruction scale and texture mip bias to the host.</summary>
    /// <param name="renderScale">Internal width divided by display width.</param>
    /// <param name="lodBias">Effective texture mip bias for the active plan.</param>
    void SetActivePlan(float renderScale, float lodBias);
    /// <summary>Clears the provider's published active plan.</summary>
    void ClearActivePlan();
    /// <summary>Returns true only for the first runtime stand-down.</summary>
    bool DisableAtRuntime();
}

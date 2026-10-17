namespace VulkanStory.Contracts;

/// <summary>Render and display dimensions selected by one reconstruction provider.</summary>
/// <param name="RenderWidth">Internal scene width in pixels.</param>
/// <param name="RenderHeight">Internal scene height in pixels.</param>
/// <param name="DisplayWidth">Reconstructed output width in pixels.</param>
/// <param name="DisplayHeight">Reconstructed output height in pixels.</param>
/// <param name="Quality">Provider quality token associated with these dimensions.</param>
/// <param name="ProviderLodBias">Explicit provider mip bias, or null to use the retained recommendation.</param>
/// <param name="LodBiasOffset">Adjustment subtracted from the ratio-derived mip bias.</param>
public readonly record struct UpscalerPlan(
    int RenderWidth, int RenderHeight, int DisplayWidth, int DisplayHeight,
    string Quality, float? ProviderLodBias = null, float LodBiasOffset = 1f)
{
    /// <summary>Whether all dimensions are positive and neither render extent exceeds its display extent.</summary>
    public bool IsValid => RenderWidth > 0 && RenderHeight > 0 && DisplayWidth > 0 &&
        DisplayHeight > 0 && RenderWidth <= DisplayWidth && RenderHeight <= DisplayHeight;

    /// <summary>Horizontal render-to-display ratio, or one for a nonpositive display width.</summary>
    public float RenderScale => DisplayWidth > 0 ? (float)RenderWidth / DisplayWidth : 1f;

    /// <summary>Provider override when supplied; otherwise the ratio-derived mip recommendation.</summary>
    public float LodBias => ProviderLodBias ?? RecommendedLodBias(RenderWidth, DisplayWidth, LodBiasOffset);

    // Preserves the original OptimumConfig formula. The game adapter supplies
    // the setting explicitly; providers never read a game or global config.
    /// <summary>Derives the retained mip bias for downscaled reconstruction.</summary>
    /// <param name="renderWidth">Internal scene width in pixels.</param>
    /// <param name="displayWidth">Output width in pixels.</param>
    /// <param name="offset">Adjustment clamped to zero through one; nonfinite values use one.</param>
    /// <returns>Zero for an invalid or unscaled ratio; otherwise log2 of the bounded ratio minus the adjustment.</returns>
    public static float RecommendedLodBias(int renderWidth, int displayWidth, float offset = 1f)
    {
        if (displayWidth <= 0) return 0f;
        float scale = (float)renderWidth / displayWidth;
        if (!float.IsFinite(scale) || scale <= 0f || scale >= 1f) return 0f;
        float clampedOffset = float.IsFinite(offset) ? Math.Clamp(offset, 0f, 1f) : 1f;
        return MathF.Log2(Math.Clamp(scale, 0.01f, 1f)) - clampedOffset;
    }
}

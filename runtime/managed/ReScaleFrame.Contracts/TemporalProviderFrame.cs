namespace VulkanStory.Contracts;

/// <summary>
/// Current rendered frame data consumed by reconstruction and frame generation.
/// The game adapter captures this after camera/jitter setup, and owns the inverse
/// view storage until every provider evaluation for this frame has returned.
/// </summary>
/// <param name="JitterX">Current horizontal projection jitter in render pixels.</param>
/// <param name="JitterY">Current vertical projection jitter in render pixels.</param>
/// <param name="Reset">Whether the provider must reset temporal history for this evaluation.</param>
/// <param name="DeltaTimeMs">Real-frame duration in milliseconds.</param>
/// <param name="NearPlane">Camera near clipping distance.</param>
/// <param name="FarPlane">Camera far clipping distance.</param>
/// <param name="FovRadians">Camera field of view in radians.</param>
/// <param name="PlayerPositionX">Current player/world camera X position.</param>
/// <param name="PlayerPositionY">Current player/world camera Y position.</param>
/// <param name="PlayerPositionZ">Current player/world camera Z position.</param>
/// <param name="InverseView">Borrowed inverse origin-view matrix storage, retained until evaluation returns.</param>
public readonly record struct TemporalProviderFrame(
    float JitterX, float JitterY, bool Reset, float DeltaTimeMs,
    float NearPlane, float FarPlane, float FovRadians,
    float PlayerPositionX, float PlayerPositionY, float PlayerPositionZ,
    ReadOnlyMemory<float> InverseView)
{
    /// <summary>FSR 3 FG needs a 4x4 inverse of the camera's origin view matrix.</summary>
    public bool HasInverseView => InverseView.Length >= 16;
}

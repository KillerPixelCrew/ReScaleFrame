using System;
namespace ReScaleFrame.Vulkan;

/// <summary>Camera transforms and vectors supplied to Streamline frame-generation constants.</summary>
/// <remarks>Matrix arrays are borrowed from the game adapter and contain 16 finite values; this value does not own or copy them.</remarks>
/// <param name="ViewToClip">Current unjittered view-to-clip transform.</param>
/// <param name="ClipToView">Inverse current view-to-clip transform.</param>
/// <param name="ClipToPreviousClip">Current clip-to-previous-clip transform.</param>
/// <param name="PreviousClipToClip">Previous clip-to-current-clip transform.</param>
/// <param name="Near">Positive camera near plane.</param>
/// <param name="Far">Camera far plane, greater than Near.</param>
/// <param name="FovRadians">Positive vertical field of view in radians.</param>
/// <param name="AspectRatio">Positive camera width-to-height ratio.</param>
/// <param name="JitterX">Current horizontal jitter in render pixels.</param>
/// <param name="JitterY">Current vertical jitter in render pixels.</param>
/// <param name="PositionX">Camera world-position X.</param>
/// <param name="PositionY">Camera world-position Y.</param>
/// <param name="PositionZ">Camera world-position Z.</param>
/// <param name="UpX">Camera up-vector X.</param>
/// <param name="UpY">Camera up-vector Y.</param>
/// <param name="UpZ">Camera up-vector Z.</param>
/// <param name="RightX">Camera right-vector X.</param>
/// <param name="RightY">Camera right-vector Y.</param>
/// <param name="RightZ">Camera right-vector Z.</param>
/// <param name="ForwardX">Camera forward-vector X.</param>
/// <param name="ForwardY">Camera forward-vector Y.</param>
/// <param name="ForwardZ">Camera forward-vector Z.</param>
internal readonly record struct NgxFrameGenerationCamera(
    float[] ViewToClip, float[] ClipToView, float[] ClipToPreviousClip,
    float[] PreviousClipToClip, float Near, float Far, float FovRadians,
    float AspectRatio, float JitterX, float JitterY,
    float PositionX, float PositionY, float PositionZ,
    float UpX, float UpY, float UpZ, float RightX, float RightY, float RightZ,
    float ForwardX, float ForwardY, float ForwardZ)
{
    /// <summary>Whether every matrix/vector is finite and the camera planes, field of view and aspect ratio are usable.</summary>
    public bool IsValid => MatrixIsValid(ViewToClip) && MatrixIsValid(ClipToView) &&
        MatrixIsValid(ClipToPreviousClip) && MatrixIsValid(PreviousClipToClip) &&
        float.IsFinite(Near) && Near > 0 && float.IsFinite(Far) && Far > Near &&
        float.IsFinite(FovRadians) && FovRadians > 0 && float.IsFinite(AspectRatio) && AspectRatio > 0 &&
        float.IsFinite(JitterX) && float.IsFinite(JitterY) &&
        float.IsFinite(PositionX) && float.IsFinite(PositionY) && float.IsFinite(PositionZ) &&
        float.IsFinite(UpX) && float.IsFinite(UpY) && float.IsFinite(UpZ) &&
        float.IsFinite(RightX) && float.IsFinite(RightY) && float.IsFinite(RightZ) &&
        float.IsFinite(ForwardX) && float.IsFinite(ForwardY) && float.IsFinite(ForwardZ);

    private static bool MatrixIsValid(float[]? matrix)
    {
        if (matrix?.Length != 16) return false;
        foreach (float value in matrix) if (!float.IsFinite(value)) return false;
        return true;
    }
}

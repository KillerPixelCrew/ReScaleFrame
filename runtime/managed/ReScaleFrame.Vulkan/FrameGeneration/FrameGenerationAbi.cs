using System.Runtime.InteropServices;

namespace ReScaleFrame.Vulkan;

/// <summary>Sequential six-word snapshot returned by the Streamline frame-generation bridge.</summary>
[StructLayout(LayoutKind.Sequential)]
internal struct StreamlineFrameGenerationState
{
    internal uint Status, Presents, MaximumGenerated, MinimumSize, VsyncSupport, DynamicMfgSupport;
}

/// <summary>Sequential C-ABI frame resources, camera matrices and shared-fence values for XeSS presentation.</summary>
[StructLayout(LayoutKind.Sequential)]
internal unsafe struct XessPresentationFrame
{
    public uint FrameId;
    public uint Reset;
    public uint Vsync;
    public uint Reserved;
    public nint Color;
    public nint Depth;
    public nint Motion;
    public nint Hudless;
    public nint Ui;
    public fixed float ViewMatrix[16];
    public fixed float ProjectionMatrix[16];
    public float JitterX, JitterY;
    public float MotionScaleX, MotionScaleY;
    public ulong ReadyFenceValue;
    public ulong DoneFenceValue;
}


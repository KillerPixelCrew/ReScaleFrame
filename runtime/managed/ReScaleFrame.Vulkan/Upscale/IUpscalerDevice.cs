using Silk.NET.Vulkan;
using VulkanStory.Contracts;

namespace ReScaleFrame.Vulkan;

/// <summary>Retained provider calls implemented by the real renderer owner.</summary>
internal interface IUpscalerDevice
{
    /// <summary>Whether a device provider may already have initialized NGX on this same logical device.</summary>
    bool HasExternalNgxInitializer => false;
    /// <summary>Creates a renderer-owned provider texture and returns its renderer ID.</summary>
    /// <param name="width">Texture width in pixels.</param>
    /// <param name="height">Texture height in pixels.</param>
    /// <param name="format">Vulkan texel format.</param>
    /// <param name="storage">Whether shader storage-image access is required.</param>
    /// <param name="pixels">Optional initial texels; borrowed for the upload operation.</param>
    /// <param name="bytesPerPixel">Initial upload texel size when pixels are supplied.</param>
    /// <returns>The texture ID used by subsequent renderer operations.</returns>
    int CreateUpscaleTexture(int width, int height, Format format, bool storage,
        IntPtr pixels = default, int bytesPerPixel = 0);
    /// <summary>Removes a renderer texture and schedules GPU-safe release through the device's resource path.</summary>
    void DeleteTexture(int textureId);
    /// <summary>Transfers disposal responsibility to the renderer's frame-timeline retirement queue.</summary>
    void RetireUpscalerResource(IDisposable resource);
    /// <summary>Records NGX DLSS feature creation using the current renderer command buffer.</summary>
    NgxResult CreateDlssFeature(in NgxDlssSettings settings, out NgxDlssFeature? feature);
    /// <summary>Transitions the supplied texture IDs and records one NGX DLSS evaluation.</summary>
    NgxResult EvaluateDlss(NgxDlssFeature feature, int color, int depth, int motion, int output,
        in NgxDlssEvaluation frame);
    /// <summary>Schedules an NGX feature for release after its referencing frames complete.</summary>
    void RetireDlssFeature(NgxDlssFeature feature);
    /// <summary>Collects resources eligible for deletion under the renderer's completion timeline.</summary>
    /// <returns>The number of collected deletion entries.</returns>
    int DrainDeferredDeletions();
    /// <summary>Converts renderer motion and records FidelityFX SR for the current frame.</summary>
    /// <returns>The native bridge result code; zero indicates success.</returns>
    int EvaluateFsr3(Fsr3Native api, nint context, int motion, in UpscalerFrame frame, bool firstFrame);
    /// <summary>Converts renderer motion and records XeSS SR for the current frame.</summary>
    /// <returns>The XeSS result code; nonnegative values include successful SDK warnings.</returns>
    int EvaluateXess(XessNative api, nint context, int color, int depth, int motion, int output,
        in UpscalerFrame frame, bool firstFrame);
    /// <summary>Creates the Vulkan/DX12 image sets and fence required by the FSR 4 plan.</summary>
    /// <param name="runtime">DX12 runtime that must outlive the returned shared resources.</param>
    /// <param name="plan">Input and output extents for the shared images.</param>
    /// <param name="shared">New resource owner on success.</param>
    /// <param name="reason">Availability or creation detail.</param>
    /// <returns>Whether the shared resources were created.</returns>
    bool TryCreateFsr4SharedFrames(Fsr4Runtime runtime, in UpscalerPlan plan,
        out IFsr4SharedFrames? shared, out string reason);
    /// <summary>Submits Vulkan input copies, dispatches FSR 4 on DX12 and records the output copy after a shared-fence wait.</summary>
    /// <returns>The native bridge result code; zero indicates success.</returns>
    int EvaluateFsr4(Fsr4Runtime runtime, IFsr4SharedFrames shared, int motion,
        in UpscalerPlan plan, in UpscalerFrame frame, bool firstFrame);
}

/// <summary>Renderer-owned image exchange retained until both APIs have drained.</summary>
internal interface IFsr4SharedFrames : IDisposable { }

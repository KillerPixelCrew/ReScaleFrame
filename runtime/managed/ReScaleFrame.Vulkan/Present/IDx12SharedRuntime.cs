using Silk.NET.Vulkan;

namespace ReScaleFrame.Vulkan;

/// <summary>Resource and fence operations shared by the DX12 vendor bridges.</summary>
internal interface IDx12SharedRuntime
{
    /// <summary>Creates a DX12 resource and an NT handle suitable for dedicated Vulkan image import.</summary>
    /// <param name="width">Image width in pixels.</param>
    /// <param name="height">Image height in pixels.</param>
    /// <param name="format">Vulkan format translated by the native bridge.</param>
    /// <param name="writable">Whether DX12 must support writing the shared image.</param>
    /// <param name="sharedHandle">Owned NT handle; the importer closes it after use.</param>
    /// <param name="resource">Owned DX12 resource reference released with <see cref="ReleaseImage"/>.</param>
    /// <returns>The bridge result code; zero indicates success.</returns>
    int CreateSharedImage(uint width, uint height, Format format, bool writable,
        out nint sharedHandle, out nint resource);
    /// <summary>Releases a DX12 image reference after both graphics APIs have finished using it.</summary>
    /// <param name="resource">DX12 resource returned by <see cref="CreateSharedImage"/>.</param>
    void ReleaseImage(nint resource);
    /// <summary>Creates the shared DX12 fence backing a Vulkan timeline-semaphore import.</summary>
    /// <param name="sharedHandle">Owned NT fence handle; the importer closes it after import.</param>
    /// <returns>The bridge result code; zero indicates success.</returns>
    int CreateSharedFence(out nint sharedHandle);
}

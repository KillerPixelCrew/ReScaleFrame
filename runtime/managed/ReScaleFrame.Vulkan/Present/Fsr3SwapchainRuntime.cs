using System;
using Silk.NET.Vulkan;

using Semaphore = Silk.NET.Vulkan.Semaphore;

namespace ReScaleFrame.Vulkan;

/// <summary>
/// One FidelityFX Vulkan frame-generation swapchain context. Its replacement
/// entry points are queried together from the SDK and remain valid until the
/// context is destroyed. The caller must finish all presents before disposal.
/// </summary>
internal sealed unsafe class Fsr3SwapchainRuntime : IDisposable
{
    private readonly Fsr3Native _api;
    private nint _context;
    private Exception? _releaseFailure;

    private Fsr3SwapchainRuntime(Fsr3Native api, nint context)
    {
        _api = api;
        _context = context;
    }

    /// <summary>Creates the FidelityFX swapchain context when the renderer has the required distinct queues.</summary>
    /// <returns>Whether the SDK created a nonzero context; reason records queue or SDK failure detail.</returns>
    public static bool TryCreate(IVulkanProviderContext vk, SwapchainCreateInfoKHR* info,
        out Fsr3SwapchainRuntime? runtime, out string reason)
    {
        runtime = null;
        if (!vk.Fsr3SwapchainQueuesAvailable)
        {
            reason = "Current FSR3 integration needs four distinct queues in the graphics/compute family; " + vk.Fsr3QueueTopology;
            return false;
        }
        Fsr3Native? api = Fsr3Native.TryLoad(out string? loadError);
        if (api == null)
        {
            reason = loadError ?? "FidelityFX Vulkan runtime unavailable";
            return false;
        }
        nint context = 0;
        int code = api.CreateSwapchain(vk.PhysicalDevice, vk.Device, vk.GraphicsQueue,
            vk.Fsr3AsyncQueue, vk.Fsr3PresentQueue, vk.Fsr3AcquireQueue,
            vk.GraphicsQueueFamily, info, &context);
        if (code != 0 || context == 0)
        {
            reason = "FidelityFX swapchain context failed (" + code + ")";
            return false;
        }
        runtime = new Fsr3SwapchainRuntime(api, context);
        reason = "ready";
        return true;
    }

    /// <summary>Provider swapchain handle, or default after context destruction.</summary>
    public SwapchainKHR Handle => _context != 0 ? _api.SwapchainHandle(_context) : default;
    /// <summary>Borrowed native provider context; zero when it is absent.</summary>
    public nint NativeContext => _context;

    /// <summary>Recreates the provider-owned swapchain while retaining its native presentation context.</summary>
    public Result Recreate(SwapchainCreateInfoKHR* info, out SwapchainKHR chain)
    {
        RequireLifetime();
        chain = default;
        if (_context == 0) return Result.ErrorInitializationFailed;
        SwapchainKHR created = default;
        Result result = _api.RecreateSwapchain(_context, info, &created);
        chain = created;
        return result;
    }

    /// <summary>Queries provider-owned swapchain images using Vulkan count/query semantics.</summary>
    public Result GetImages(Device device, SwapchainKHR chain, ref uint count, Image* images)
    {
        RequireLifetime();
        if (_context == 0) return Result.ErrorInitializationFailed;
        fixed (uint* countPtr = &count)
            return _api.GetSwapchainImages(_context, chain, countPtr, images);
    }

    /// <summary>Acquires a provider image with an infinite timeout and the supplied Vulkan synchronization.</summary>
    public Result Acquire(Device device, SwapchainKHR chain, Semaphore semaphore, Fence fence,
        ref uint index)
    {
        RequireLifetime();
        if (_context == 0) return Result.ErrorInitializationFailed;
        fixed (uint* indexPtr = &index)
            return _api.AcquireSwapchain(_context, chain, ulong.MaxValue, semaphore, fence, indexPtr);
    }

    /// <summary>Presents exclusively through the FidelityFX swapchain context.</summary>
    public Result Present(Queue queue, PresentInfoKHR* info)
    {
        RequireLifetime();
        return _context != 0 ? _api.PresentSwapchain(_context, queue, info) : Result.ErrorInitializationFailed;
    }

    /// <summary>Disables and drains provider presentation before releasing the supplied swapchain.</summary>
    public void PrepareDestroy(SwapchainKHR chain)
    {
        RequireLifetime();
        if (_context != 0) CheckRelease(_api.PrepareSwapchainDestroy(_context, chain), "disable/drain before swapchain release");
    }
    /// <summary>Destroys the specified provider swapchain after release preparation succeeds.</summary>
    public void DestroyChain(SwapchainKHR chain)
    {
        RequireLifetime();
        if (_context != 0) CheckRelease(_api.DestroySwapchainChain(_context, chain), "swapchain destruction");
    }
    private void CheckRelease(int result, string operation)
    {
        if (result == 0) return;
        var failure = new InvalidOperationException("FidelityFX " + operation + " failed (" + result + "); ownership retained.");
        _releaseFailure = failure;
        throw failure;
    }
    /// <summary>Throws after a checked provider release failure so remaining native ownership is retained.</summary>
    internal void RequireLifetime()
    {
        if (_releaseFailure != null)
            throw new InvalidOperationException("FidelityFX release failed; cleanup is terminal.", _releaseFailure);
    }

    /// <summary>Returns the native provider-reported present count, or zero when no context is active.</summary>
    public ulong LastPresentCount(SwapchainKHR chain) => _context != 0 ?
        _api.SwapchainLastPresentCount(_context, chain) : 0;

    /// <inheritdoc/>
    public void Dispose()
    {
        RequireLifetime();
        if (_context == 0) return;
        CheckRelease(_api.DestroySwapchain(_context), "swapchain context destruction");
        _context = 0;
    }
}

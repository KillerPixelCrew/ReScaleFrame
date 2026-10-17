using System;
using System.Runtime.InteropServices;

namespace ReScaleFrame.Vulkan;

/// <summary>
/// The managed side of <c>native/ngx</c>: VulkanStory's own
/// <c>libVulkanStoryNgx.so</c> / <c>VulkanStoryNgx.dll</c>, which is the only call site
/// NGX accepts.
///
/// NGX resolves its caller's module from its own return address. A .NET
/// P/Invoke stub is JIT-compiled into anonymous memory, so that lookup yields a
/// null path and the driver aborts the process inside libstdc++ (DLSS spike,
/// 2026-09-12, driver 615.71.09; isolated with
/// <c>scripts/dev/ngx-probe.c</c>). Calls therefore go managed -> shim -> NGX,
/// and the shim's frame is inside a real shared object.
///
/// The shim itself never throws and never aborts: it dlopen()s the NGX runtime
/// lazily, dlsym()s every entry point, and answers
/// <see cref="NgxResult.FailShimRuntimeMissing" /> or
/// <see cref="NgxResult.FailShimEntryPointMissing" /> rather than calling into
/// nothing. So "is NGX available" is an ordinary capability question here, with
/// no risk to the process.
/// </summary>
internal static unsafe class NgxShim
{
    /// <summary>
    /// The base name passed to <c>DllImport</c>. The renderer's single native
    /// resolver selects its private RID directory before a provider call.
    /// </summary>
    public const string LibraryName = "VulkanStoryNgx";

    /// <summary>Points at an explicit shim file, for a build output that is not beside the assembly.</summary>
    public const string PathVariable = "VULKANSTORY_NGX_SHIM_PATH";

    /// <summary>
    /// <c>VULKANSTORY_NGX_SHIM_VERSION</c> this managed code is written against. The
    /// shim reports its own through <see cref="Version" />; a mismatch makes NGX
    /// unavailable instead of calling a stale binary with changed signatures.
    /// </summary>
    public const uint ExpectedVersion = 1;

    static NgxShim() => VulkanNativeLibrary.EnsureRegistered();

    private static bool _probed;
    private static uint _version;
    private static string _diagnosis = "";

    /// <summary>The shim's reported <c>VULKANSTORY_NGX_SHIM_VERSION</c>, or 0 if it could not be loaded.</summary>
    public static uint Version { get { Probe(); return _version; } }

    /// <summary>Whether the shim loaded and speaks <see cref="ExpectedVersion" />.</summary>
    public static bool IsAvailable { get { Probe(); return _version == ExpectedVersion; } }

    /// <summary>One line for a log or a test report saying what the shim is or why it is not usable.</summary>
    public static string Diagnosis { get { Probe(); return _diagnosis; } }

    /// <summary>
    /// Whether the NGX runtime itself (the driver library) could be dlopen()ed
    /// by the shim. Safe to call with no NVIDIA driver installed.
    /// </summary>
    public static NgxResult LoadRuntime()
    {
        if (!IsAvailable) return NgxResult.FailShimMissing;
        try { return LoadRuntimeCore(); }
        catch (DllNotFoundException) { return NgxResult.FailShimMissing; }
        catch (EntryPointNotFoundException) { return NgxResult.FailShimEntryPointMissing; }
    }

    /// <summary>The runtime file name the shim looks for, or "" when the shim is unusable.</summary>
    public static string RuntimeName =>
        IsAvailable ? Marshal.PtrToStringUTF8(RuntimeNameCore()) ?? "" : "";

    /// <summary>The dynamic loader's message from the shim's last failed load, or "".</summary>
    public static string LastLoadError =>
        IsAvailable ? Marshal.PtrToStringUTF8(LastLoadErrorCore()) ?? "" : "";

    private static void Probe()
    {
        if (_probed) return;
        _probed = true;
        try
        {
            _version = VersionCore();
            _diagnosis = _version == ExpectedVersion
                ? "NGX shim " + LibraryName + " v" + _version + " loaded from " +
                    (VulkanNativeLibrary.NgxPath ?? "default native search")
                : "NGX shim " + LibraryName + " reports version " + _version + ", this build expects " +
                  ExpectedVersion + "; rebuild native/ngx - NGX stays unavailable.";
        }
        catch (Exception exception) when (
            exception is DllNotFoundException or EntryPointNotFoundException or BadImageFormatException)
        {
            _version = 0;
            _diagnosis = "NGX shim " + LibraryName + " could not be loaded (" +
                exception.GetType().Name + ": " + exception.Message.Trim() +
                "); build native/ngx or set " + PathVariable + ". NGX is unavailable.";
        }
    }

    // ------------------------------------------------------------------ P/Invoke

    private const CallingConvention Cdecl = CallingConvention.Cdecl;

    [DllImport(LibraryName, EntryPoint = "VulkanStoryNgx_Version", CallingConvention = Cdecl)]
    private static extern uint VersionCore();

    [DllImport(LibraryName, EntryPoint = "VulkanStoryNgx_LoadRuntime", CallingConvention = Cdecl)]
    private static extern NgxResult LoadRuntimeCore();

    [DllImport(LibraryName, EntryPoint = "VulkanStoryNgx_RuntimeName", CallingConvention = Cdecl)]
    private static extern IntPtr RuntimeNameCore();

    [DllImport(LibraryName, EntryPoint = "VulkanStoryNgx_LastLoadError", CallingConvention = Cdecl)]
    private static extern IntPtr LastLoadErrorCore();

    [DllImport(LibraryName, EntryPoint = "VulkanStoryNgx_VulkanInitProjectId", CallingConvention = Cdecl)]
    public static extern NgxResult InitProjectId(
        IntPtr projectId, NgxEngineType engineType, IntPtr engineVersion, IntPtr applicationDataPath,
        IntPtr instance, IntPtr physicalDevice, IntPtr device, int sdkVersion,
        NgxFeatureCommonInfo* featureInfo);

    /// <summary>Shuts down NGX for the supplied logical device after its features are released.</summary>
    [DllImport(LibraryName, EntryPoint = "VulkanStoryNgx_VulkanShutdown", CallingConvention = Cdecl)]
    public static extern NgxResult Shutdown(IntPtr device);

    [DllImport(LibraryName, EntryPoint = "VulkanStoryNgx_GetCapabilityParameters", CallingConvention = Cdecl)]
    public static extern NgxResult GetCapabilityParameters(out IntPtr parameters);

    [DllImport(LibraryName, EntryPoint = "VulkanStoryNgx_AllocateParameters", CallingConvention = Cdecl)]
    public static extern NgxResult AllocateParameters(out IntPtr parameters);

    [DllImport(LibraryName, EntryPoint = "VulkanStoryNgx_DestroyParameters", CallingConvention = Cdecl)]
    public static extern NgxResult DestroyParameters(IntPtr parameters);

    [DllImport(LibraryName, EntryPoint = "VulkanStoryNgx_GetFeatureRequirements", CallingConvention = Cdecl)]
    public static extern NgxResult GetFeatureRequirements(
        IntPtr instance, IntPtr physicalDevice, NgxFeatureDiscoveryInfo* discovery,
        NgxFeatureRequirement* outSupported);

    [DllImport(LibraryName, EntryPoint = "VulkanStoryNgx_GetFeatureInstanceExtensionRequirements",
        CallingConvention = Cdecl)]
    public static extern NgxResult GetFeatureInstanceExtensionRequirements(
        NgxFeatureDiscoveryInfo* discovery, uint* outCount, NgxExtensionProperties** outProperties);

    [DllImport(LibraryName, EntryPoint = "VulkanStoryNgx_GetFeatureDeviceExtensionRequirements",
        CallingConvention = Cdecl)]
    public static extern NgxResult GetFeatureDeviceExtensionRequirements(
        IntPtr instance, IntPtr physicalDevice, NgxFeatureDiscoveryInfo* discovery,
        uint* outCount, NgxExtensionProperties** outProperties);

    /// <summary>Creates an NGX feature through the shim using a live command buffer and parameter map.</summary>
    [DllImport(LibraryName, EntryPoint = "VulkanStoryNgx_CreateFeature", CallingConvention = Cdecl)]
    public static extern NgxResult CreateFeature(
        IntPtr commandBuffer, NgxFeature feature, IntPtr parameters, out IntPtr handle);

    /// <summary>Creates an NGX feature through the device-explicit shim entry point.</summary>
    [DllImport(LibraryName, EntryPoint = "VulkanStoryNgx_CreateFeature1", CallingConvention = Cdecl)]
    public static extern NgxResult CreateFeature1(
        IntPtr device, IntPtr commandBuffer, NgxFeature feature, IntPtr parameters, out IntPtr handle);

    /// <summary>Records evaluation of a live NGX feature using the supplied command buffer and parameters.</summary>
    [DllImport(LibraryName, EntryPoint = "VulkanStoryNgx_EvaluateFeature", CallingConvention = Cdecl)]
    public static extern NgxResult EvaluateFeature(
        IntPtr commandBuffer, IntPtr handle, IntPtr parameters, IntPtr progressCallback);

    /// <summary>Releases an NGX feature after its referencing GPU work has completed.</summary>
    [DllImport(LibraryName, EntryPoint = "VulkanStoryNgx_ReleaseFeature", CallingConvention = Cdecl)]
    public static extern NgxResult ReleaseFeature(IntPtr handle);

    /// <summary>Queries NGX optimal/minimum/maximum input dimensions and recommended sharpness for DLSS quality.</summary>
    [DllImport(LibraryName, EntryPoint = "VulkanStoryNgx_DlssGetOptimalSettings", CallingConvention = Cdecl)]
    public static extern NgxResult DlssGetOptimalSettings(
        IntPtr parameters, uint displayWidth, uint displayHeight, NgxPerfQuality quality,
        uint* outOptimalWidth, uint* outOptimalHeight, uint* outMinWidth, uint* outMinHeight,
        uint* outMaxWidth, uint* outMaxHeight, float* outSharpness);

    // Parameter accessors. NVSDK_NGX_Parameter is a C++ class with no C
    // accessors in the driver, so the shim owns the vtable dispatch; no slot
    // number appears in managed code any more.

    /// <summary>Sets a unsigned 64-bit integer NGX parameter through the native shim.</summary>
    /// <remarks>The supplied parameter-map pointer must remain live for the call.</remarks>
    [DllImport(LibraryName, EntryPoint = "VulkanStoryNgx_ParameterSetULongLong", CallingConvention = Cdecl)]
    public static extern NgxResult SetULong(IntPtr parameters, [MarshalAs(UnmanagedType.LPUTF8Str)] string name, ulong value);

    /// <summary>Sets a single-precision value NGX parameter through the native shim.</summary>
    /// <remarks>The supplied parameter-map pointer must remain live for the call.</remarks>
    [DllImport(LibraryName, EntryPoint = "VulkanStoryNgx_ParameterSetFloat", CallingConvention = Cdecl)]
    public static extern NgxResult SetFloat(IntPtr parameters, [MarshalAs(UnmanagedType.LPUTF8Str)] string name, float value);

    /// <summary>Sets a double-precision value NGX parameter through the native shim.</summary>
    /// <remarks>The supplied parameter-map pointer must remain live for the call.</remarks>
    [DllImport(LibraryName, EntryPoint = "VulkanStoryNgx_ParameterSetDouble", CallingConvention = Cdecl)]
    public static extern NgxResult SetDouble(IntPtr parameters, [MarshalAs(UnmanagedType.LPUTF8Str)] string name, double value);

    /// <summary>Sets a unsigned 32-bit integer NGX parameter through the native shim.</summary>
    /// <remarks>The supplied parameter-map pointer must remain live for the call.</remarks>
    [DllImport(LibraryName, EntryPoint = "VulkanStoryNgx_ParameterSetUInt", CallingConvention = Cdecl)]
    public static extern NgxResult SetUInt(IntPtr parameters, [MarshalAs(UnmanagedType.LPUTF8Str)] string name, uint value);

    /// <summary>Sets a signed 32-bit integer NGX parameter through the native shim.</summary>
    /// <remarks>The supplied parameter-map pointer must remain live for the call.</remarks>
    [DllImport(LibraryName, EntryPoint = "VulkanStoryNgx_ParameterSetInt", CallingConvention = Cdecl)]
    public static extern NgxResult SetInt(IntPtr parameters, [MarshalAs(UnmanagedType.LPUTF8Str)] string name, int value);

    /// <summary>Sets a borrowed native resource pointer NGX parameter through the native shim.</summary>
    /// <remarks>The supplied parameter-map pointer must remain live for the call.</remarks>
    [DllImport(LibraryName, EntryPoint = "VulkanStoryNgx_ParameterSetVoidPointer", CallingConvention = Cdecl)]
    public static extern NgxResult SetVoidPointer(IntPtr parameters, [MarshalAs(UnmanagedType.LPUTF8Str)] string name, IntPtr value);

    /// <summary>Reads a unsigned 64-bit integer NGX parameter through the native shim.</summary>
    /// <returns>The native NGX accessor result.</returns>
    [DllImport(LibraryName, EntryPoint = "VulkanStoryNgx_ParameterGetULongLong", CallingConvention = Cdecl)]
    public static extern NgxResult GetULong(IntPtr parameters, [MarshalAs(UnmanagedType.LPUTF8Str)] string name, out ulong value);

    /// <summary>Reads a single-precision value NGX parameter through the native shim.</summary>
    /// <returns>The native NGX accessor result.</returns>
    [DllImport(LibraryName, EntryPoint = "VulkanStoryNgx_ParameterGetFloat", CallingConvention = Cdecl)]
    public static extern NgxResult GetFloat(IntPtr parameters, [MarshalAs(UnmanagedType.LPUTF8Str)] string name, out float value);

    /// <summary>Reads a double-precision value NGX parameter through the native shim.</summary>
    /// <returns>The native NGX accessor result.</returns>
    [DllImport(LibraryName, EntryPoint = "VulkanStoryNgx_ParameterGetDouble", CallingConvention = Cdecl)]
    public static extern NgxResult GetDouble(IntPtr parameters, [MarshalAs(UnmanagedType.LPUTF8Str)] string name, out double value);

    /// <summary>Reads a unsigned 32-bit integer NGX parameter through the native shim.</summary>
    /// <returns>The native NGX accessor result.</returns>
    [DllImport(LibraryName, EntryPoint = "VulkanStoryNgx_ParameterGetUInt", CallingConvention = Cdecl)]
    public static extern NgxResult GetUInt(IntPtr parameters, [MarshalAs(UnmanagedType.LPUTF8Str)] string name, out uint value);

    /// <summary>Reads a signed 32-bit integer NGX parameter through the native shim.</summary>
    /// <returns>The native NGX accessor result.</returns>
    [DllImport(LibraryName, EntryPoint = "VulkanStoryNgx_ParameterGetInt", CallingConvention = Cdecl)]
    public static extern NgxResult GetInt(IntPtr parameters, [MarshalAs(UnmanagedType.LPUTF8Str)] string name, out int value);

    /// <summary>Reads a borrowed native resource pointer NGX parameter through the native shim.</summary>
    /// <returns>The native NGX accessor result.</returns>
    [DllImport(LibraryName, EntryPoint = "VulkanStoryNgx_ParameterGetVoidPointer", CallingConvention = Cdecl)]
    public static extern NgxResult GetVoidPointer(IntPtr parameters, [MarshalAs(UnmanagedType.LPUTF8Str)] string name, out IntPtr value);
}

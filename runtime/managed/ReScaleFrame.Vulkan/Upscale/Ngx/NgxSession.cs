using System;
using System.Collections.Generic;
using System.IO;
using System.Runtime.InteropServices;
using System.Runtime.CompilerServices;
using System.Text;

namespace ReScaleFrame.Vulkan;

/// <summary>
/// Everything NGX needs to identify this VulkanStory process and find the feature libraries,
/// kept alive as unmanaged memory for as long as NGX might read it.
///
/// The project id is our own GUID: <c>NVSDK_NGX_VULKAN_Init_ProjectID</c> with
/// <see cref="NgxEngineType.Custom" /> needs no NVIDIA-issued application id,
/// and the driver only validates that the string is GUID-shaped.
///
/// The feature libraries (<c>libnvidia-ngx-dlss.so.*</c>,
/// <c>libnvidia-ngx-dlssg.so.*</c>) are NVIDIA redistributables and are never
/// checked into this repository: the search directory comes from
/// <see cref="FeaturePathVariable" /> or VulkanStory's private native RID
/// directory, and everything NGX-related is skipped when none exists.
/// </summary>
internal sealed unsafe class NgxSession : IDisposable
{
    /// <summary>The working custom-engine project GUID retained during the port.</summary>
    public const string ProjectId = "6f8f6ad4-3f47-4a2c-9c1e-1a5f2c0d7b31";

    /// <summary>Directory holding the NGX feature libraries.</summary>
    public const string FeaturePathVariable = "VULKANSTORY_NGX_FEATURE_PATH";

    private readonly List<IntPtr> _scratch = new();
    private readonly object _gate = new();
    private IntPtr _projectId;
    private IntPtr _engineVersion;
    private IntPtr _applicationDataPath;
    private NgxFeatureCommonInfo* _featureInfo;
    private bool _disposed;

    public NgxSession(string engineVersion, string applicationDataPath, IReadOnlyList<string> featurePaths)
    {
        EngineVersionString = engineVersion;
        ApplicationDataPath = applicationDataPath;
        FeaturePaths = featurePaths;

        _projectId = Utf8(ProjectId);
        _engineVersion = Utf8(engineVersion);
        _applicationDataPath = Wide(applicationDataPath);

        // wchar_t const* const* Path: an array of pointers to UTF-32 strings on
        // Linux, where wchar_t is four bytes. Getting this wrong is silent - NGX
        // simply fails to find the feature library.
        IntPtr pathArray = Alloc(IntPtr.Size * Math.Max(featurePaths.Count, 1));
        for (int i = 0; i < featurePaths.Count; i++)
        {
            ((IntPtr*)pathArray)[i] = Wide(featurePaths[i]);
        }

        _featureInfo = (NgxFeatureCommonInfo*)Alloc(sizeof(NgxFeatureCommonInfo));
        *_featureInfo = default;
        _featureInfo->PathListInfo.Path = pathArray;
        _featureInfo->PathListInfo.Length = (uint)featurePaths.Count;
        if (Environment.GetEnvironmentVariable("VULKANSTORY_NGX_LOG") == "1")
        {
            _featureInfo->LoggingInfo.LoggingCallback =
                (IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, int, int, void>)&LogNgx;
            _featureInfo->LoggingInfo.MinimumLoggingLevel = 2;
        }
    }

    public string EngineVersionString { get; }
    public string ApplicationDataPath { get; }
    public IReadOnlyList<string> FeaturePaths { get; }

    [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
    private static void LogNgx(IntPtr message, int level, int feature)
    {
        // Exceptions cannot cross a native callback boundary.
        try
        {
            string? line = Marshal.PtrToStringUTF8(message);
            if (line != null) Console.Error.WriteLine("[NGX " + feature + "/" + level + "] " + line);
        }
        catch { }
    }

    /// <summary>A discovery record for one feature, pointing at this session's strings.</summary>
    private NgxFeatureDiscoveryInfo Discovery(NgxFeature feature)
    {
        lock (_gate)
        {
            ThrowIfDisposed();
            return new NgxFeatureDiscoveryInfo
            {
                SdkVersion = NgxInterop.VersionApi,
                FeatureId = feature,
                Identifier = new NgxApplicationIdentifier
                {
                    IdentifierType = 1, // NVSDK_NGX_Application_Identifier_Type_Project_Id
                    ProjectDesc = new NgxProjectIdDescription
                    {
                        ProjectId = _projectId,
                        EngineType = NgxEngineType.Custom,
                        EngineVersion = _engineVersion,
                    },
                },
                ApplicationDataPath = _applicationDataPath,
                FeatureInfo = (IntPtr)_featureInfo,
            };
        }
    }

    /// <summary>
    /// The instance extensions NGX reports for a feature. Needs no VkInstance,
    /// so it can run before the instance is created - which is the point.
    /// </summary>
    public NgxResult InstanceExtensions(NgxFeature feature, out List<string> extensions)
    {
        extensions = new List<string>();
        lock (_gate)
        {
            ThrowIfDisposed();
            if (!NgxInterop.ManagedCallSiteIsSupported) return NgxResult.FailNotImplemented;

            NgxFeatureDiscoveryInfo discovery = Discovery(feature);
            uint count = 0;
            NgxExtensionProperties* properties = null;
            NgxResult result = NgxInterop.GetFeatureInstanceExtensionRequirements(&discovery, &count, &properties);
            if (NgxInterop.Succeeded(result) && !Collect(count, properties, extensions))
                return NgxResult.FailInvalidParameter;
            return result;
        }
    }

    /// <summary>The device extensions NGX reports for a feature on this adapter.</summary>
    public NgxResult DeviceExtensions(
        IntPtr instance, IntPtr physicalDevice, NgxFeature feature, out List<string> extensions)
    {
        extensions = new List<string>();
        lock (_gate)
        {
            ThrowIfDisposed();
            if (!NgxInterop.ManagedCallSiteIsSupported) return NgxResult.FailNotImplemented;

            NgxFeatureDiscoveryInfo discovery = Discovery(feature);
            uint count = 0;
            NgxExtensionProperties* properties = null;
            NgxResult result = NgxInterop.GetFeatureDeviceExtensionRequirements(
                instance, physicalDevice, &discovery, &count, &properties);
            if (NgxInterop.Succeeded(result) && !Collect(count, properties, extensions))
                return NgxResult.FailInvalidParameter;
            return result;
        }
    }

    /// <summary>
    /// NVSDK_NGX_VULKAN_GetFeatureRequirements: whether the feature is supported
    /// at all, which is answerable before NGX is initialised.
    /// </summary>
    public NgxResult FeatureRequirements(
        IntPtr instance, IntPtr physicalDevice, NgxFeature feature,
        out NgxFeatureSupport supported, out uint minHwArchitecture, out string minOsVersion)
    {
        supported = NgxFeatureSupport.CheckNotPresent;
        minHwArchitecture = 0;
        minOsVersion = "";
        lock (_gate)
        {
            ThrowIfDisposed();
            if (!NgxInterop.ManagedCallSiteIsSupported) return NgxResult.FailNotImplemented;

            NgxFeatureDiscoveryInfo discovery = Discovery(feature);
            NgxFeatureRequirement requirement = default;
            NgxResult result = NgxInterop.GetFeatureRequirements(
                instance, physicalDevice, &discovery, &requirement);
            supported = requirement.FeatureSupported;
            minHwArchitecture = requirement.MinHwArchitecture;
            minOsVersion = Marshal.PtrToStringUTF8((IntPtr)requirement.MinOsVersion) ?? "";
            return result;
        }
    }

    /// <summary>
    /// Initialises NGX for one Vulkan device. The driver's exported
    /// <c>Init_ProjectID</c> takes the SDK version before the feature info; see
    /// <see cref="NgxInterop" /> for why that differs from the header.
    /// </summary>
    public NgxResult Initialize(IntPtr instance, IntPtr physicalDevice, IntPtr device)
    {
        lock (_gate)
        {
            ThrowIfDisposed();
            if (!NgxInterop.ManagedCallSiteIsSupported) return NgxResult.FailNotImplemented;

            // Best effort, exactly as the host's own preparation does it: NGX writes only
            // logs and caches there, and an unwritable path is not worth refusing the
            // feature over - let alone failing InitializeGraphics, which is what an
            // exception here would do, falling the whole client back to OpenGL.
            try
            {
                if (!string.IsNullOrEmpty(ApplicationDataPath)) Directory.CreateDirectory(ApplicationDataPath);
            }
            catch (IOException)
            {
            }
            catch (UnauthorizedAccessException)
            {
            }
            return NgxInterop.InitProjectId(
                _projectId, NgxEngineType.Custom, _engineVersion, _applicationDataPath,
                instance, physicalDevice, device, NgxInterop.VersionApi, _featureInfo);
        }
    }

    // There is deliberately no Shutdown here. NVSDK_NGX_VULKAN_Shutdown1 has exactly
    // one caller in this process - NgxLifetime.ShutDown - which performs the release
    // and the drain that must precede it and refuses to run twice. A session that
    // could shut NGX down on its own would be a second way to get the order wrong.

    /// <summary>
    /// The C# equivalent of <c>NGX_DLSS_GET_OPTIMAL_SETTINGS</c> from
    /// <c>nvsdk_ngx_helpers.h</c>: the header's helper is inline C that reads a
    /// callback out of the capability parameters and calls it, so there is
    /// nothing to link against.
    /// </summary>
    public static NgxResult OptimalSettings(
        NgxParameters parameters, uint displayWidth, uint displayHeight, NgxPerfQuality quality,
        out NgxOptimalSettings settings)
    {
        settings = default;
        if (parameters.IsNull) return NgxResult.FailInvalidParameter;

        if (!NgxInterop.UseDirectCalls)
        {
            // The callback NGX hands out lives inside libnvidia-ngx, so calling
            // it has the same return-address problem as every other entry
            // point: the shim reads it out of the parameter block and calls it.
            if (!NgxShim.IsAvailable) return NgxResult.FailShimMissing;

            uint shimWidth = 0, shimHeight = 0;
            uint shimMinWidth = 0, shimMinHeight = 0, shimMaxWidth = 0, shimMaxHeight = 0;
            float shimSharpness = 0;
            NgxResult shimResult = NgxShim.DlssGetOptimalSettings(
                parameters.Handle, displayWidth, displayHeight, quality,
                &shimWidth, &shimHeight, &shimMinWidth, &shimMinHeight,
                &shimMaxWidth, &shimMaxHeight, &shimSharpness);
            if (!NgxInterop.Succeeded(shimResult)) return shimResult;
            settings = new NgxOptimalSettings(
                shimWidth, shimHeight, shimMinWidth, shimMinHeight,
                shimMaxWidth, shimMaxHeight, shimSharpness);
            return shimResult;
        }

        NgxResult lookup = parameters.GetVoidPointer(
            NgxParameterNames.DlssOptimalSettingsCallback, out IntPtr callback);
        if (callback == IntPtr.Zero)
        {
            // The header's own diagnosis: an out-of-date feature library, or
            // parameters from AllocateParameters instead of GetCapabilityParameters.
            return NgxInterop.Succeeded(lookup) ? NgxResult.FailOutOfDate : lookup;
        }

        parameters.SetUInt(NgxParameterNames.Width, displayWidth);
        parameters.SetUInt(NgxParameterNames.Height, displayHeight);
        parameters.SetInt(NgxParameterNames.PerfQualityValue, (int)quality);
        parameters.SetInt(NgxParameterNames.RtxValue, 0);

        var invoke = (delegate* unmanaged[Cdecl]<IntPtr, NgxResult>)callback;
        NgxResult result = invoke(parameters.Handle);
        if (!NgxInterop.Succeeded(result)) return result;

        parameters.GetUInt(NgxParameterNames.OutWidth, out uint optimalWidth);
        parameters.GetUInt(NgxParameterNames.OutHeight, out uint optimalHeight);

        uint maxWidth = optimalWidth, maxHeight = optimalHeight;
        uint minWidth = optimalWidth, minHeight = optimalHeight;
        if (NgxInterop.Succeeded(parameters.GetUInt(NgxParameterNames.DlssGetDynamicMaxRenderWidth, out uint value)))
            maxWidth = value;
        if (NgxInterop.Succeeded(parameters.GetUInt(NgxParameterNames.DlssGetDynamicMaxRenderHeight, out value)))
            maxHeight = value;
        if (NgxInterop.Succeeded(parameters.GetUInt(NgxParameterNames.DlssGetDynamicMinRenderWidth, out value)))
            minWidth = value;
        if (NgxInterop.Succeeded(parameters.GetUInt(NgxParameterNames.DlssGetDynamicMinRenderHeight, out value)))
            minHeight = value;
        parameters.GetFloat(NgxParameterNames.Sharpness, out float sharpness);

        settings = new NgxOptimalSettings(
            optimalWidth, optimalHeight, minWidth, minHeight, maxWidth, maxHeight, sharpness);
        return result;
    }

    /// <summary>
    /// What the SDK's own wrapper substitutes when the driver answers
    /// <see cref="NgxResult.FailNotImplemented" /> to the instance query - read
    /// out of the wrapper's static data, which is the only place it is written
    /// down. Driver 615.71.09 always takes this path on Linux.
    /// </summary>
    public static readonly IReadOnlyList<string> InstanceExtensionFallback =
        new[] { "VK_KHR_get_physical_device_properties2" };

    /// <summary>The same fallback for the device query.</summary>
    public static readonly IReadOnlyList<string> DeviceExtensionFallback = new[]
    {
        "VK_NVX_binary_import",
        "VK_NVX_image_view_handle",
        "VK_KHR_buffer_device_address",
        "VK_KHR_push_descriptor",
    };

    /// <summary>
    /// Where the NGX feature libraries are, or an empty list. Checked in order:
    /// <see cref="FeaturePathVariable" />, then VulkanStory's private native
    /// package directory. Development builds may also use their output folder.
    /// </summary>
    public static IReadOnlyList<string> FindFeaturePaths()
    {
        var found = new List<string>();
        foreach (string candidate in FeaturePathCandidates())
        {
            if (!Directory.Exists(candidate)) continue;
            string pattern = OperatingSystem.IsWindows() ? "nvngx_dlss.dll" : "libnvidia-ngx-*.so*";
            if (Directory.GetFiles(candidate, pattern).Length == 0) continue;
            string full = Path.GetFullPath(candidate);
            if (!found.Contains(full)) found.Add(full);
        }
        return found;
    }

    /// <summary>
    /// Every directory <see cref="FindFeaturePaths" /> looks in, in order and
    /// before any of them is tested for existence.
    ///
    /// The released layout is VulkanStory/native/&lt;rid&gt;, optionally with a
    /// dlss child directory. A deployed payload never searches the game folder
    /// or another mod for NVIDIA redistributables. Development output paths are
    /// accepted only when this is not the deployed payload.
    /// </summary>
    internal static IReadOnlyList<string> FeaturePathCandidates()
    {
        var candidates = new List<string>();
        string? configured = Environment.GetEnvironmentVariable(FeaturePathVariable);
        if (!string.IsNullOrWhiteSpace(configured) && Path.IsPathFullyQualified(configured))
            candidates.Add(Path.GetFullPath(configured));

        string? packaged = NativeRuntimePaths.PackageNativeDirectory;
        if (packaged is not null)
        {
            candidates.Add(packaged);
            candidates.Add(Path.Combine(packaged, "dlss"));
        }
        if (!NativeRuntimePaths.IsDeployedPayload)
        {
            string assembly = NativeRuntimePaths.AssemblyDirectory;
            candidates.Add(assembly);
            candidates.Add(Path.Combine(assembly, "dlss"));
            string application = AppContext.BaseDirectory;
            if (!string.Equals(application, assembly, StringComparison.OrdinalIgnoreCase))
            {
                candidates.Add(application);
                candidates.Add(Path.Combine(application, "dlss"));
            }
        }
        return candidates;
    }

    private static bool Collect(uint count, NgxExtensionProperties* properties, List<string> into)
    {
        // A corrupt count from a mismatched NGX ABI must never index arbitrarily
        // far into driver-owned memory. Real extension lists are much smaller.
        if (count > 256 || (count != 0 && properties == null)) return false;
        for (uint i = 0; i < count; i++) into.Add(properties[i].Name);
        return true;
    }

    private void ThrowIfDisposed()
    {
        if (_disposed) throw new ObjectDisposedException(nameof(NgxSession));
    }

    private IntPtr Utf8(string value)
    {
        byte[] bytes = Encoding.UTF8.GetBytes(value);
        IntPtr memory = Alloc(bytes.Length + 1);
        Marshal.Copy(bytes, 0, memory, bytes.Length);
        Marshal.WriteByte(memory, bytes.Length, 0);
        return memory;
    }

    /// <summary>A null-terminated platform wchar_t string (UTF-16 on Windows, UTF-32 on Unix).</summary>
    private IntPtr Wide(string value)
    {
        bool windows = OperatingSystem.IsWindows();
        byte[] bytes = windows ? Encoding.Unicode.GetBytes(value) : Encoding.UTF32.GetBytes(value);
        int terminatorBytes = windows ? 2 : 4;
        IntPtr memory = Alloc(bytes.Length + terminatorBytes);
        Marshal.Copy(bytes, 0, memory, bytes.Length);
        for (int i = 0; i < terminatorBytes; i++) Marshal.WriteByte(memory, bytes.Length + i, 0);
        return memory;
    }

    private IntPtr Alloc(int bytes)
    {
        IntPtr memory = Marshal.AllocHGlobal(bytes);
        _scratch.Add(memory);
        return memory;
    }

    /// <inheritdoc/>
    public void Dispose()
    {
        lock (_gate)
        {
            if (_disposed) return;
            _disposed = true;
            _featureInfo = null;
            _projectId = IntPtr.Zero;
            _engineVersion = IntPtr.Zero;
            _applicationDataPath = IntPtr.Zero;
            for (int i = 0; i < _scratch.Count; i++) Marshal.FreeHGlobal(_scratch[i]);
            _scratch.Clear();
        }
    }
}

/// <summary>What NGX_DLSS_GET_OPTIMAL_SETTINGS returns for one quality preset.</summary>
internal readonly struct NgxOptimalSettings
{
    public NgxOptimalSettings(
        uint optimalWidth, uint optimalHeight, uint minWidth, uint minHeight,
        uint maxWidth, uint maxHeight, float sharpness)
    {
        OptimalWidth = optimalWidth;
        OptimalHeight = optimalHeight;
        MinWidth = minWidth;
        MinHeight = minHeight;
        MaxWidth = maxWidth;
        MaxHeight = maxHeight;
        Sharpness = sharpness;
    }

    public uint OptimalWidth { get; }
    public uint OptimalHeight { get; }
    public uint MinWidth { get; }
    public uint MinHeight { get; }
    public uint MaxWidth { get; }
    public uint MaxHeight { get; }
    public float Sharpness { get; }

    /// <inheritdoc/>
    public override string ToString() =>
        OptimalWidth + "x" + OptimalHeight +
        " (min " + MinWidth + "x" + MinHeight + ", max " + MaxWidth + "x" + MaxHeight +
        ", sharpness " + Sharpness.ToString("0.###") + ")";
}

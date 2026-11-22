// SPDX-License-Identifier: MIT
// Expected x64 census layouts, compared with the independently compiled SDK facts.
use std::ffi::c_void;
use std::slice;

type Fields = &'static [(&'static str, usize)];

const SL: Fields = &[
    ("sl::BaseStructure size", 32),
    ("sl::Preferences::showConsole", 32),
    ("sl::Preferences::logLevel", 36),
    ("sl::Preferences::numPathsToPlugins", 48),
    ("sl::Preferences::pathToLogsAndData", 56),
    ("sl::Preferences::flags", 88),
    ("sl::Preferences::featuresToLoad", 96),
    ("sl::Preferences::numFeaturesToLoad", 104),
    ("sl::Preferences::applicationId", 108),
    ("sl::Preferences::engine", 112),
    ("sl::Preferences::engineVersion", 120),
    ("sl::Preferences::projectId", 128),
    ("sl::Preferences::renderAPI", 136),
    ("sl::ResourceTag::resource", 32),
    ("sl::ResourceTag::type", 40),
    ("sl::ResourceTag::lifecycle", 44),
    ("sl::ResourceTag::extent", 48),
    ("sl::ResourceTag size", 64),
    ("sl::Resource::type", 32),
    ("sl::Resource::native", 40),
    ("sl::Resource::state", 64),
    ("sl::Resource::width", 68),
    ("sl::Resource::height", 72),
    ("sl::Resource::nativeFormat", 76),
    ("sl::Constants::jitterOffset", 352),
    ("sl::Constants::cameraViewToClip", 32),
    ("sl::Constants::clipToCameraView", 96),
    ("sl::Constants::clipToLensClip", 160),
    ("sl::Constants::clipToPrevClip", 224),
    ("sl::Constants::prevClipToClip", 288),
    ("sl::Constants::cameraPos", 376),
    ("sl::Constants::cameraUp", 388),
    ("sl::Constants::cameraRight", 400),
    ("sl::Constants::cameraFwd", 412),
    ("sl::Constants::mvecScale", 360),
    ("sl::Constants::cameraNear", 424),
    ("sl::Constants::cameraFar", 428),
    ("sl::Constants::cameraFOV", 432),
    ("sl::Constants::cameraAspectRatio", 436),
    ("sl::Constants::depthInverted", 444),
    ("sl::Constants::cameraMotionIncluded", 445),
    ("sl::Constants::motionVectors3D", 446),
    ("sl::Constants::reset", 447),
    ("sl::Constants::orthographicProjection", 448),
    ("sl::Constants::motionVectorsDilated", 449),
    ("sl::Constants::motionVectorsJittered", 450),
    ("sl::DLSSGOptions::mode", 32),
    ("sl::DLSSGOptions::numFramesToGenerate", 36),
    ("sl::DLSSGOptions::flags", 40),
    ("sl::DLSSGOptions::dynamicResWidth", 44),
    ("sl::DLSSGOptions::numBackBuffers", 52),
    ("sl::DLSSGOptions::mvecDepthWidth", 56),
    ("sl::DLSSGOptions::colorWidth", 64),
    ("sl::DLSSGOptions::colorBufferFormat", 72),
    ("sl::DLSSGOptions::uiBufferFormat", 88),
    ("sl::DLSSOptions::mode", 32),
    ("sl::DLSSOptions::outputWidth", 36),
    ("sl::DLSSOptions::outputHeight", 40),
    ("sl::DLSSOptions::sharpness", 44),
    ("sl::DLSSOptions::preExposure", 48),
    ("sl::DLSSOptions::exposureScale", 52),
    ("sl::ReflexOptions::mode", 32),
    ("sl::ReflexOptions::frameLimitUs", 36),
    ("sl::ReflexOptions::useMarkersToOptimize", 40),
    ("sl::ReflexOptions::virtualKey", 42),
    ("sl::ReflexOptions::idThread", 44),
    ("sl::ViewportHandle size", 40),
];

const FFX: Fields = &[
    ("FfxApiResource size", 48),
    ("ffxDispatchDescUpscale::commandList", 16),
    ("ffxDispatchDescUpscale::color", 24),
    ("ffxDispatchDescUpscale::output", 312),
    ("ffxDispatchDescUpscale::jitterOffset", 360),
    ("ffxDispatchDescUpscale::renderSize", 376),
    ("ffxDispatchDescUpscale::upscaleSize", 384),
    ("ffxDispatchDescUpscale::enableSharpening", 392),
    ("ffxDispatchDescUpscale::sharpness", 396),
    ("ffxDispatchDescUpscale::reset", 408),
    ("ffxDispatchDescUpscale::cameraNear", 412),
    ("ffxDispatchDescUpscale::viewSpaceToMetersFactor", 424),
    ("ffxDispatchDescUpscale::flags", 428),
    ("ffxDispatchDescUpscale size", 432),
    ("ffxDispatchDescFrameGeneration::presentColor", 24),
    ("ffxDispatchDescFrameGeneration::numGeneratedFrames", 264),
    ("ffxDispatchDescFrameGeneration::reset", 268),
    ("ffxDispatchDescFrameGeneration::frameID", 304),
    ("ffxDispatchDescFrameGeneration size", 312),
    ("ffxConfigureDescFrameGeneration::swapChain", 16),
    (
        "ffxConfigureDescFrameGeneration::frameGenerationEnabled",
        56,
    ),
    ("ffxConfigureDescFrameGeneration::HUDLessColor", 64),
    ("ffxConfigureDescFrameGeneration::flags", 112),
    ("ffxConfigureDescFrameGeneration::frameID", 136),
    ("ffxDispatchDescFrameGenerationPrepareV2::frameID", 16),
    ("ffxDispatchDescFrameGenerationPrepareV2::commandList", 32),
    ("ffxDispatchDescFrameGenerationPrepareV2::renderSize", 40),
    ("ffxDispatchDescFrameGenerationPrepareV2::depth", 88),
    ("ffxCreateContextDescUpscale::flags", 16),
    ("ffxCreateContextDescUpscale::maxRenderSize", 20),
    ("ffxCreateContextDescUpscale::maxUpscaleSize", 28),
    ("ffxCreateContextDescFrameGeneration::flags", 16),
    ("ffxCreateContextDescFrameGeneration::displaySize", 20),
    ("ffxCreateContextDescFrameGeneration::maxRenderSize", 28),
    ("ffxCreateContextDescFrameGeneration::backBufferFormat", 36),
    (
        "ffxCreateContextDescFrameGenerationSwapChainWrapDX12::swapchain",
        16,
    ),
    (
        "ffxCreateContextDescFrameGenerationSwapChainWrapDX12::gameQueue",
        24,
    ),
    (
        "ffxCreateContextDescFrameGenerationSwapChainNewDX12::desc",
        24,
    ),
    (
        "ffxCreateContextDescFrameGenerationSwapChainNewDX12::gameQueue",
        40,
    ),
    (
        "ffxCreateContextDescFrameGenerationSwapChainForHwndDX12::hwnd",
        24,
    ),
    (
        "ffxCreateContextDescFrameGenerationSwapChainForHwndDX12::desc",
        32,
    ),
    (
        "ffxCreateContextDescFrameGenerationSwapChainForHwndDX12::gameQueue",
        56,
    ),
    ("ffxCreateBackendDX12Desc::device", 16),
    ("FFX_API_DISPATCH_DESC_TYPE_UPSCALE", 0x00010001),
    ("FFX_API_DISPATCH_DESC_TYPE_FRAMEGENERATION", 0x00020003),
    ("FFX_API_CONFIGURE_DESC_TYPE_FRAMEGENERATION", 0x00020002),
    (
        "FFX_API_DISPATCH_DESC_TYPE_FRAMEGENERATION_PREPARE_V2",
        0x0002000c,
    ),
    (
        "FFX_API_CREATE_CONTEXT_DESC_TYPE_FRAMEGENERATIONSWAPCHAIN_NEW_DX12",
        0x00030005,
    ),
    (
        "FFX_API_CREATE_CONTEXT_DESC_TYPE_FRAMEGENERATIONSWAPCHAIN_FOR_HWND_DX12",
        0x00030006,
    ),
    ("FFX_API_CREATE_CONTEXT_DESC_TYPE_BACKEND_DX12", 0x00000002),
];

const XESS: Fields = &[
    ("xess_d3d12_execute_params_t::pColorTexture", 0),
    ("xess_d3d12_execute_params_t::pOutputTexture", 40),
    ("xess_d3d12_execute_params_t::jitterOffsetX", 48),
    ("xess_d3d12_execute_params_t::exposureScale", 56),
    ("xess_d3d12_execute_params_t::resetHistory", 60),
    ("xess_d3d12_execute_params_t::inputWidth", 64),
    ("xess_d3d12_execute_params_t::inputColorBase", 72),
    ("xess_d3d12_execute_params_t::pDescriptorHeap", 120),
    ("xess_d3d12_init_params_t::outputResolution", 0),
    ("xess_d3d12_init_params_t::qualitySetting", 8),
    ("xess_d3d12_init_params_t::initFlags", 12),
    ("xess_d3d12_init_params_t::creationNodeMask", 16),
    ("xess_d3d12_init_params_t::visibleNodeMask", 20),
];

const VK: Fields = &[
    ("VkInstanceCreateInfo::pApplicationInfo", 24),
    ("VkInstanceCreateInfo::enabledLayerCount", 32),
    ("VkInstanceCreateInfo::ppEnabledLayerNames", 40),
    ("VkInstanceCreateInfo::enabledExtensionCount", 48),
    ("VkInstanceCreateInfo::ppEnabledExtensionNames", 56),
    ("VkApplicationInfo::pApplicationName", 16),
    ("VkApplicationInfo::applicationVersion", 24),
    ("VkApplicationInfo::pEngineName", 32),
    ("VkApplicationInfo::engineVersion", 40),
    ("VkApplicationInfo::apiVersion", 44),
    ("VkDeviceCreateInfo::queueCreateInfoCount", 20),
    ("VkDeviceCreateInfo::pQueueCreateInfos", 24),
    ("VkDeviceCreateInfo::enabledLayerCount", 32),
    ("VkDeviceCreateInfo::enabledExtensionCount", 48),
    ("VkDeviceCreateInfo::ppEnabledExtensionNames", 56),
    ("VkDeviceQueueCreateInfo size", 40),
    ("VkDeviceQueueCreateInfo::queueFamilyIndex", 20),
    ("VkDeviceQueueCreateInfo::queueCount", 24),
    ("VkDeviceQueueInfo2::queueFamilyIndex", 20),
    ("VkDeviceQueueInfo2::queueIndex", 24),
    ("VkSubmitInfo size", 72),
    ("VkSubmitInfo::waitSemaphoreCount", 16),
    ("VkSubmitInfo::commandBufferCount", 40),
    ("VkSubmitInfo::signalSemaphoreCount", 56),
    ("VkSubmitInfo2 size", 64),
    ("VkSubmitInfo2::waitSemaphoreInfoCount", 20),
    ("VkSubmitInfo2::commandBufferInfoCount", 32),
    ("VkSubmitInfo2::signalSemaphoreInfoCount", 48),
    ("VkPresentInfoKHR::waitSemaphoreCount", 16),
    ("VkPresentInfoKHR::swapchainCount", 32),
    ("VkPresentInfoKHR::pSwapchains", 40),
    ("VkPresentInfoKHR::pImageIndices", 48),
    ("VkSwapchainCreateInfoKHR::surface", 24),
    ("VkSwapchainCreateInfoKHR::minImageCount", 32),
    ("VkSwapchainCreateInfoKHR::imageFormat", 36),
    ("VkSwapchainCreateInfoKHR::imageColorSpace", 40),
    ("VkSwapchainCreateInfoKHR::imageExtent", 44),
    ("VkSwapchainCreateInfoKHR::imageArrayLayers", 52),
    ("VkSwapchainCreateInfoKHR::imageUsage", 56),
    ("VkSwapchainCreateInfoKHR::imageSharingMode", 60),
    ("VkSwapchainCreateInfoKHR::preTransform", 80),
    ("VkSwapchainCreateInfoKHR::compositeAlpha", 84),
    ("VkSwapchainCreateInfoKHR::presentMode", 88),
    ("VkSwapchainCreateInfoKHR::clipped", 92),
    ("VkSwapchainCreateInfoKHR::oldSwapchain", 96),
];

const FSR2: Fields = &[
    ("FfxResource size", 184),
    ("Fsr212::FfxFsr2DispatchDescription::color", 8),
    ("Fsr212::FfxFsr2DispatchDescription::output", 1112),
    ("Fsr212::FfxFsr2DispatchDescription::jitterOffset", 1296),
    ("Fsr212::FfxFsr2DispatchDescription::renderSize", 1312),
    ("Fsr212::FfxFsr2DispatchDescription::enableSharpening", 1320),
    ("Fsr212::FfxFsr2DispatchDescription::reset", 1336),
    (
        "Fsr212::FfxFsr2DispatchDescription::cameraFovAngleVertical",
        1348,
    ),
    ("FfxFsr2DispatchDescription size", 1352),
    ("Fsr212::FfxResource::name", 8),
    ("Fsr212::FfxResource::description", 136),
    ("Fsr212::FfxResource::state", 164),
    ("Fsr212::FfxResource::isDepth", 168),
];

const SDK: [(&str, Fields); 5] = [
    ("Streamline", SL),
    ("FidelityFX", FFX),
    ("XeSS", XESS),
    ("Vulkan", VK),
    ("FSR 2.1.2", FSR2),
];
type Visit = unsafe extern "C" fn(*mut c_void, u32, *const usize, usize);
type Collect = unsafe extern "C" fn(*mut c_void, Visit);

#[derive(Default)]
struct Counts {
    sets: u32,
    failures: u32,
}

unsafe extern "C" fn visit(context: *mut c_void, sdk: u32, facts: *const usize, count: usize) {
    let counts: &mut Counts = unsafe { &mut *context.cast() };
    let Some((name, expected)) = SDK.get(sdk as usize) else {
        counts.failures += 1;
        return;
    };
    counts.sets += 1;
    if count != expected.len() {
        println!(
            "FAIL {name}: header supplied {count} facts, census expects {}",
            expected.len()
        );
        counts.failures += 1;
        return;
    }
    let facts = unsafe { slice::from_raw_parts(facts, count) };
    for (&actual, &(field, wanted)) in facts.iter().zip(*expected) {
        if actual != wanted {
            println!("FAIL {field}: header says {actual}, census uses {wanted}");
            counts.failures += 1;
        }
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_test_census_layouts(collect: Collect) -> i32 {
    let mut counts = Counts::default();
    unsafe { collect((&raw mut counts).cast(), visit) };
    if counts.sets == 0 {
        println!("no vendor headers found; nothing was checked");
        return 77;
    }
    println!(
        "vendor_census_layouts: {} header set(s) checked, {} failure(s)",
        counts.sets, counts.failures
    );
    i32::from(counts.failures != 0)
}

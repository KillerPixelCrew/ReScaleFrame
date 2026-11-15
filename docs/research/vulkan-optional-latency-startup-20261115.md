# Optional latency-only Vulkan startup without Streamline

15 November 2026. Question: a generic latency request currently calls rsf_slvk_initialize with no DLSS feature bits, but a missing/unsupported Streamline session made Vulkan startup fail before AMD or XeLL could own pacing. The correction is generic core policy for features0; no TotK vendor detection, public ABI change or pinned submodule modification is involved.

## Failure ownership review

The real runtime/vulkan/streamline/bridge.cpp, VulkanStorySlInitializeFeatures records loadedHere before loading the interposer. Missing exports or a failed slInit clear SDK feature functions and free a module loaded by that attempt. A retained older module is not freed. Its Vulkan dispatch members are deliberately not part of clearSdkFunctions, but can therefore contain stale addresses when loadedHere was freed; the generic owner must not assume that the bridge resolver remains usable after unsuccessful initialization.

The same function refuses a persistent failed shutdown before attempting another initialization. VulkanStorySlShutdownChecked preserves that error and retains the module, and clears SDK feature functions even when shutdown fails. Therefore any failed initialization now gets checked cleanup before optional fallback. A failed cleanup is retained as rollback_failed, startup returns FEATURE_FAILED, reinitialization refuses and WSI SDK-facing calls are blocked. No native fallback is claimed safe in that state. Exceptions similarly attempt checked cleanup and return failure. The fixture's recoverable mock cleanup tests the owner state machine; the real bridge's sticky failed shutdown may remain permanently refused for that process.

## Runtime behavior

native_latency_vulkan.cpp distinguishes startup ownership from successful SDK initialization. For features0 it attempts Streamline/PCL when a directory is supplied. After clean refusal, or when Streamline is not compiled/a directory is absent, it initializes a generic session with no Reflex/PCL availability. Explicit DLSS SR or FG bits still fail when unavailable. Invalid ABI, arguments or UTF8 remain errors rather than silently corrected settings.

The fallback resolves vkGetInstanceProcAddr from actual system32 vulkan-1.dll using LoadLibraryExW with LOAD_LIBRARY_SEARCH_SYSTEM32. That module is retained for process lifetime, including after runtime shutdown while device/instance destruction can still use its dispatch. A missing real Vulkan loader fails startup rather than returning a fake or null resolver. Resolver source is selected on successful startup: retained interposer dispatch for a successful SDK session, retained system loader dispatch for native-only startup. Hosts must still finish old Vulkan object lifetimes before initializing a different renderer session.

rsf_slvk_invoke refuses SDK leases when only generic startup exists. Native WSI callbacks and Present use their actual supplied dispatch directly without calling an unavailable bridge. SDK-ready WSI continues through the existing bridge; failed rollback refuses WSI callbacks. Marker helpers also check initialized/live SDK state, including after failed shutdown, rather than touching cleared SDK functions. The SDK resolver is used only after a proven successful initialization and remains available after its normal checked shutdown for destruction, per the existing bridge contract.

Core source/input/render/Present facts remain active without Streamline. Existing generic owner selection can use AMD or the leased XeSS presentation/XeLL owner. No Reflex sleep or PCL bind is added to either fallback owner. Native-only sessions reject duplicate initialization, cleanly shut down, retain native dispatch and can reinitialize. Missing Reflex is reported as unavailable ownership/options instead of a Vulkan startup failure. Original SDK refusal remains in bounded status.

## Validation and scope

Changed only runtime/orchestrator/src/native_latency_vulkan.cpp, tests/native_latency_vulkan.cpp and this new note. No camera ABI3, Skyrim, TotK, common helper, pinned or shared build paths were edited. No staging, commits, remote actions, game/GPU/UI runs or new public exports occurred.

One final isolated batched check produced build/checks/rsf-optional-latency-worker/{sdk,amd,no-sdk}.exe in the TotK workspace. Full successful commands are in its commands.txt. All executables returned0:

- sdk.exe: actual native_latency_vulkan/native_cpu functions with controlled Streamline refusal, explicit required DLSS refusal, checked cleanup poison/retry, retained real system resolver, direct native WSI/Present callbacks and no unavailable SDK lease. Existing PCL queue/epoch and borrowed-context lifetime tests remain passing. The XeLL fallback receives an actual accepted input boundary and sleeps once without any Reflex sleep/PCL bind.
- amd.exe: same actual core and tests with test-only AMD endpoint stubs (RSF_TEST_LATENCY_AMD_STUBS) instead of latency_amd_vulkan.cpp. Generic AMD receives the accepted input fact after clean optional refusal, with no Reflex sleep or PCL bind. This validates orchestration selection, not Anti-Lag driver/device support.
- no-sdk.exe: actual core compiled without RSF_HAVE_STREAMLINE_VK. Optional startup, actual system loader export lookup, duplicate-init refusal, retained post-shutdown resolver, strict requested-DLSS refusal and native reinitialization pass.
- Complete changed core source compiles with -Wall -Wextra -Werror -Wno-missing-field-initializers both with and without Streamline; scoped diff checks pass. The system loader was loaded and its global vkCreateInstance export resolved, but no Vulkan instance/device/GPU operation was invoked by these checks. Existing CPU tests use their prior invisible message-only window solely for queue filtering, not GUI automation.

Coordinator owns native full verification, selective publication/pin, TotK rebuild and actual non-DLSS hardware startup/latency acceptance. Successful CPU routing does not establish AMD/XeLL hardware availability or latency benefit.

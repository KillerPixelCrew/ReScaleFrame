// SPDX-License-Identifier: MIT
// Compiles the enabled SDK/Vulkan leaves. All callbacks are synthetic.
#include "../runtime/orchestrator/src/native_fg_dlss_vulkan.cpp"
#include "../runtime/orchestrator/src/native_fg_dlss_vulkan_capture.cpp"
#include <cstdio>
#include <cstdlib>
#include "vulkan-fg/dlss_abi.h"

namespace {
rsf_dlss_fixture_state& fixture() { return *rsf_fixture_dlss_state(); }
uint32_t sdk_token = 42;
const auto prepare_list = reinterpret_cast<void*>(10);
const auto color_list = reinterpret_cast<void*>(11);

void require(bool condition, const char* message) {
    if (!condition) { std::fprintf(stderr, "%s\n", message); std::exit(1); }
}

sl::Result options(const sl::ViewportHandle& view, const sl::DLSSGOptions& value) {
    require(static_cast<uint32_t>(view) == 0 && value.numBackBuffers == 3, "FG uses root viewport zero and enumerated image count");
    require(value.queueParallelismMode == sl::DLSSGQueueParallelismMode::eBlockNoClientQueues,
        "SDK never queues a future timeline wait ahead of client binary waits");
    require(value.enableUserInterfaceRecomposition == sl::Boolean::eFalse, "missing UI producer does not enable recomposition");
    require(value.structVersion == sl::kStructVersion5 && value.bReserved15 == sl::Boolean::eInvalid &&
        value.dynamicTargetFrameRate == 0.0f && value.onErrorCallback == on_error,
        "packed options keep the pinned defaults and native API-error callback");
    require((static_cast<uint32_t>(value.flags) & static_cast<uint32_t>(sl::DLSSGFlags::eRetainResourcesWhenOff)) != 0,
        "Off keeps resources available for checked retirement");
    if (fixture().refuse_options) return sl::Result::eErrorInvalidParameter;
    if (value.mode == sl::DLSSGMode::eOn) {
        ++fixture().on_calls;
        require(value.numFramesToGenerate == 3, "MFG forwards generated count excluding real frame");
    } else {
        ++fixture().off_calls;
        require(value.numFramesToGenerate == 1, "Off publishes the SDK baseline count");
    }
    return sl::Result::eOk;
}
sl::Result state(const sl::ViewportHandle&, sl::DLSSGState& value, const sl::DLSSGOptions*) {
    value.numFramesToGenerateMax = fixture().max_generated; value.minWidthOrHeight = 128;
    value.numFramesActuallyPresented = fixture().sdk_presented; fixture().sdk_presented = 0;
    value.inputsProcessingCompletionFence = reinterpret_cast<void*>(0xc9);
    value.lastPresentInputsProcessingCompletionFenceValue = fixture().completion_value;
    return sl::Result::eOk;
}
sl::Result set_constants(const sl::Constants& data, const sl::FrameToken& token, const sl::ViewportHandle&) {
    require(&token == reinterpret_cast<const sl::FrameToken*>(&sdk_token), "constants borrow the original SDK token");
    require(data.mvecScale.x == 0.5f && data.mvecScale.y == -0.5f, "motion scale uses the render dimensions");
    require(data.structVersion == sl::kStructVersion2 && data.cameraAspectRatio == 1280.0f / 720.0f &&
        data.clipToLensClip[0].x == sl::INVALID_FLOAT && data.cameraMotionIncluded == sl::Boolean::eTrue,
        "packed camera keeps SDK defaults and the presented aspect ratio");
    require(data.prevClipToClip[0].x == 1.0f && data.prevClipToClip[3].w == 1.0f,
        "native inverse fills the packed camera before dispatch");
    return sl::Result::eOk;
}
sl::Result set_tags(const sl::FrameToken& token, const sl::ViewportHandle&, const sl::ResourceTag* tags, uint32_t count, sl::CommandBuffer* commands) {
    require(&token == reinterpret_cast<const sl::FrameToken*>(&sdk_token) && count == 2, "tags borrow the original SDK token");
    require(tags[0].lifecycle == sl::ResourceLifecycle::eOnlyValidNow && tags[0].resource &&
        reinterpret_cast<uint64_t>(tags[0].resource->native) >= 1000, "tag uses an owned Vulkan image, not the engine image");
    if (commands == reinterpret_cast<sl::CommandBuffer*>(prepare_list)) {
        require(tags[0].type == sl::kBufferTypeDepth && tags[1].type == sl::kBufferTypeMotionVectors, "prepare uses typed guide tags");
        require(tags[0].extent.width == 640 && tags[0].extent.height == 360 &&
            tags[1].extent == tags[0].extent && tags[1].lifecycle == sl::ResourceLifecycle::eOnlyValidNow,
            "both staged guides carry the render extent and copy lifetime");
    } else {
        require(commands == reinterpret_cast<sl::CommandBuffer*>(color_list) && tags[0].type == sl::kBufferTypeHUDLessColor &&
            tags[1].type == sl::kBufferTypeUIColorAndAlpha && !tags[1].resource, "HUD-less explicitly clears absent UI recomposition input");
        require(tags[0].extent.width == 1280 && tags[0].extent.height == 720 &&
            tags[1].lifecycle == sl::ResourceLifecycle::eValidUntilPresent && !tags[1].extent,
            "HUD-less uses the presented extent and the cleared UI tag has no extent");
    }
    return fixture().refuse_tags ? sl::Result::eErrorInvalidParameter : sl::Result::eOk;
}
sl::Result free_resources(sl::Feature feature, const sl::ViewportHandle& view) {
    require(feature == sl::kFeatureDLSS_G && static_cast<uint32_t>(view) == 0, "release addresses the shared viewport");
    ++fixture().release_calls;
    return fixture().refuse_release ? sl::Result::eErrorInvalidParameter : sl::Result::eOk;
}
rsf_backend_result fake_initialize(void*, uintptr_t, void* user) {
    const auto& call = *static_cast<rsf_dlgvk_sdk_call*>(user);
    auto& self = *static_cast<Native*>(call.native);
    self.constants = set_constants; self.tags = set_tags; self.release = free_resources;
    self.options = options; self.state = state;
    return RSF_BACKEND_OK;
}

// Runs the Rust present capture owner with the real Vulkan, WSI and
// registration leaves. Only open, frame selection, readback recording and
// file output are synthetic.
rsf::CaptureVk* synthetic_capture = nullptr;
int32_t open_present_capture(const rsf_fgvk_setup& setup, rsf_dlss_capture** capture) {
    static const auto synthetic = [] {
        auto result = rsf::capture_platform;
        result.open = [](const rsf_fgvk_setup*, void** out) -> rsf_backend_result { *out = synthetic_capture; return RSF_BACKEND_OK; };
        result.wants = [](void*, uint64_t frame) -> uint32_t { return frame == 42; };
        result.readbacks = rsf_fixture_dlss_readbacks;
        result.record = rsf_fixture_dlss_record;
        result.flush = rsf_fixture_dlss_flush;
        result.failure = rsf_fixture_dlss_failure;
        result.append = rsf_fixture_dlss_append;
        return result;
    }();
    synthetic_capture = new rsf::CaptureVk;
    auto& vk = *synthetic_capture;
    vk.setup = setup; vk.registration = rsf_fixture_dlss_register_capture;
    vk.commands = rsf_vk_capture_commands_open(&vk.setup.graphics);
    require(vk.commands != nullptr, "synthetic capture resolves its command and fence operations");
    rsf_fixture_dlss_reset_waited_fence();
    return rsf_core_dlss_capture_open(&synthetic, &setup, capture);
}
}
extern "C" rsf_backend_result rsf_slvk_invoke(const rsf_slvk_call* call) {
    require(!rsf_fixture_dlss_locked() && call->struct_size == sizeof(*call) && call->abi_version == RSF_SLVK_ABI_VERSION,
        "worker receives versioned call outside the host queue lock");
    if (call->session_id && (call->session_id != fixture().lease_session || call->frame_id != fixture().lease_frame)) return RSF_BACKEND_ERROR_NOT_READY;
    return call->callback(reinterpret_cast<void*>(1), call->frame_id ? reinterpret_cast<uintptr_t>(&sdk_token) : 0, call->user);
}
extern "C" rsf_backend_result rsf_fixture_dlss_create(rsf_dlss_fixture_variant variant, const rsf_fgvk_setup* setup,
    rsf_dlssgvk** owner, uint64_t* chain_out) {
    // The provider borrows the selected platform table until destruction.
    static const auto synthetic = [] { auto result = platform; result.initialize = fake_initialize; return result; }();
    static const auto capture_required = [] {
        auto result = synthetic;
        result.prepare_chain = [](void* native, uint32_t* capture_enabled, void** gpu) {
            const auto result = platform.prepare_chain(native, capture_enabled, gpu);
            if (result == RSF_BACKEND_OK) *capture_enabled = 1;
            return result;
        };
        return result;
    }();
    static const auto capture_refused = [] {
        auto result = synthetic;
        result.start_capture = [](void*) { return RSF_BACKEND_ERROR_FEATURE_FAILED; };
        return result;
    }();
    const auto& selected = variant == RSF_DLSS_FIXTURE_CAPTURE_REQUIRED ? capture_required :
        variant == RSF_DLSS_FIXTURE_CAPTURE_REFUSED ? capture_refused : synthetic;
    return rsf_core_dlgvk_create(&selected, setup, owner, chain_out);
}
extern "C" int32_t rsf_fixture_dlss_capture_open(const rsf_fgvk_setup* setup, rsf_dlss_capture** capture,
    rsf_dlss_fixture_capture_calls* calls) {
    rsf_fixture_dlss_capture_calls_init(calls);
    return open_present_capture(*setup, capture);
}
int main() {
    SetEnvironmentVariableW(L"RSF_VULKAN_FRAME_CAPTURE_DIR", nullptr);
    return rsf_fixture_dlss_run();
}

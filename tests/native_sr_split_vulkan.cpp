// SPDX-License-Identifier: MIT
#if !defined(RSF_TEST_SPLIT_STUB) && !defined(RSF_TEST_SPLIT_DISABLED)
// The runtime's existing PUBLIC build definition also reaches its consumers.
// This test deliberately checks the DLL import declarations instead.
#undef RSF_RUNTIME_BUILD
#endif
#include <rescaleframe/native_sr_split_vulkan.h>
#include <cstdio>
#include <cstddef>
#include <stdexcept>
#include "../runtime/orchestrator/src/native_sr_split_lease.h"

namespace {
int failures = 0;
void check(bool value, const char* message) {
    if (!value) { std::printf("FAIL: %s\n", message); ++failures; }
}
rsf_vk_function no_proc(void*, const char*) { return nullptr; }
int32_t split(void*, void*, uint64_t, uint64_t, uint64_t, uint64_t, void**) { return 0; }
}

#if defined(RSF_TEST_SPLIT_STUB)
#include "../runtime/orchestrator/src/native_sr_fsr4_vulkan.h"
struct rsf_fsr4vk {};
namespace {
rsf_fsr4vk instance;
int32_t provider_result = 0;
bool throw_evaluate = false, throw_info = false;
unsigned creates = 0, evaluations = 0;
const rsf_srvk_setup* forwarded_setup = nullptr;
const rsf_fsr4vk_handoff* forwarded_handoff = nullptr;
}
int32_t rsf_fsr4vk_create(const rsf_srvk_setup* setup, const rsf_fsr4vk_handoff* handoff, rsf_fsr4vk** out) {
    ++creates; forwarded_setup = setup; forwarded_handoff = handoff;
    *out = &instance; return provider_result;
}
int32_t rsf_fsr4vk_plan(const rsf_fsr4vk*, uint32_t* width, uint32_t* height) {
    *width = 320; *height = 180; return provider_result;
}
int32_t rsf_fsr4vk_evaluate(rsf_fsr4vk*, void*, const rsf_srvk_frame*, void** continuation) {
    ++evaluations;
    *continuation = reinterpret_cast<void*>(2);
    if (throw_evaluate) throw std::runtime_error("provider exception");
    return provider_result;
}
int32_t rsf_fsr4vk_info(const rsf_fsr4vk* owner, rsf_fsr4_bridge_info* info) {
    check(owner == &instance && info->struct_size == sizeof(*info) && info->abi_version == RSF_FSR4_BRIDGE_ABI_VERSION,
        "public info uses actual private query with bridge ABI");
    if (throw_info) throw std::runtime_error("info exception");
    info->adapter_luid = 0x123456789abcdef0ull; info->provider_version = 0x456789abcdef0123ull;
    info->vendor_id = 0x10de; info->int8_compatibility = 1;
    return provider_result;
}
int32_t rsf_fsr4vk_destroy(rsf_fsr4vk*) { return provider_result; }
void rsf_fsr4vk_retire_commands(rsf_fsr4vk*,void*) {}
int32_t rsf_fsr4vk_submitted(rsf_fsr4vk*,void*) { return provider_result; }
#endif

#if defined(RSF_TEST_SPLIT_STUB) || defined(RSF_TEST_SPLIT_DISABLED)
#include "../runtime/orchestrator/src/native_sr_split_vulkan.cpp"
#endif

int main() {
    rsf::sr::SplitLease lease;
    void* producer = reinterpret_cast<void*>(10);
    void* resumed = reinterpret_cast<void*>(11);
    lease.begin(producer);
    check(!lease.pending() && !lease.submitted(producer), "legacy submission contract unchanged");
    lease.tracked = true; lease.begin(producer); lease.continuation = resumed;
    check(lease.pending(), "recorded work retains owner before submission");
    check(!lease.submitted(resumed) && lease.pending(), "out of order continuation refused");
    check(!lease.submitted(reinterpret_cast<void*>(12)), "unrelated submission cannot release lease");
    check(lease.submitted(producer) && lease.pending(), "producer alone cannot release continuation");
    check(!lease.submitted(producer), "duplicate notification refused");
    check(lease.submitted(resumed) && !lease.pending(), "both successful submissions release CPU admission");
    lease.begin(producer);
    check(lease.pending(), "failed split retains partially recorded producer");
    auto* fake = reinterpret_cast<rsf_fsr4vk*>(1);
    void* commands = reinterpret_cast<void*>(1);
    rsf_srvk_setup setup{};
    setup.struct_size = sizeof(setup); setup.abi_version = RSF_SRVK_ABI_VERSION;
    setup.instance = setup.physical_device = setup.device = commands;
    setup.instance_proc = setup.device_proc = no_proc;
    setup.backend = RSF_SR_FSR4; setup.quality = 1;
    setup.output_width = 640; setup.output_height = 360;
    setup.runtime_directory_utf8 = "C:/unused";
    rsf_fsr4vk_handoff handoff{sizeof(handoff), RSF_SRVK_SPLIT_ABI_VERSION, 1, 0, nullptr, split};
    rsf_fsr4vk* owner = fake;
    check(rsf_srvk_fsr4_create(&setup, &handoff, nullptr) == RSF_BACKEND_ERROR_INVALID_ARGUMENT, "required owner output");
    auto bad_setup = setup; bad_setup.struct_size = sizeof(setup) - 1; bad_setup.abi_version = 99;
    check(rsf_srvk_fsr4_create(&bad_setup, &handoff, &owner) == RSF_BACKEND_ERROR_INVALID_ARGUMENT && !owner, "short setup checked before version and clears owner");
    bad_setup = setup; bad_setup.abi_version = 99;
    check(rsf_srvk_fsr4_create(&bad_setup, &handoff, &owner) == RSF_BACKEND_ERROR_ABI_MISMATCH, "setup version");
    auto bad_handoff = handoff; bad_handoff.struct_size = sizeof(handoff) - 1; bad_handoff.abi_version = 99;
    check(rsf_srvk_fsr4_create(&setup, &bad_handoff, &owner) == RSF_BACKEND_ERROR_INVALID_ARGUMENT, "short handoff before version");
    bad_handoff = handoff; bad_handoff.abi_version = 99;
    check(rsf_srvk_fsr4_create(&setup, &bad_handoff, &owner) == RSF_BACKEND_ERROR_ABI_MISMATCH, "handoff version");
    bad_handoff = handoff; bad_handoff.timeline_enabled = 0;
    check(rsf_srvk_fsr4_create(&setup, &bad_handoff, &owner) == RSF_BACKEND_ERROR_INVALID_ARGUMENT, "timeline capability required");
    bad_handoff = handoff; bad_handoff.queue_family = 0xfffffffeu;
    check(rsf_srvk_fsr4_create(&setup, &bad_handoff, &owner) == RSF_BACKEND_ERROR_INVALID_ARGUMENT, "external queue family refused");
    bad_handoff = handoff; bad_handoff.split_record = nullptr;
    check(rsf_srvk_fsr4_create(&setup, &bad_handoff, &owner) == RSF_BACKEND_ERROR_INVALID_ARGUMENT, "split callback required");
    bad_setup = setup; bad_setup.backend = RSF_SR_FSR3;
    check(rsf_srvk_fsr4_create(&bad_setup, &handoff, &owner) == RSF_BACKEND_ERROR_NOT_SUPPORTED, "no provider substitution");
    bad_setup = setup; bad_setup.depth_inverted = 2;
    check(rsf_srvk_fsr4_create(&bad_setup, &handoff, &owner) == RSF_BACKEND_ERROR_INVALID_ARGUMENT, "boolean capability validation");
    rsf_srvk_fsr4_provider_info info{sizeof(info), RSF_SRVK_FSR4_INFO_ABI_VERSION, 1, 2, 3, 4};
    check(rsf_srvk_fsr4_info(nullptr, &info) == RSF_BACKEND_ERROR_INVALID_ARGUMENT && !info.provider_version && !info.vendor_id,
        "null info owner clears known facts");
    check(rsf_srvk_fsr4_info(fake, nullptr) == RSF_BACKEND_ERROR_INVALID_ARGUMENT, "null info output");
    info.struct_size--; info.adapter_luid = 7; info.abi_version = 99;
    check(rsf_srvk_fsr4_info(fake, &info) == RSF_BACKEND_ERROR_INVALID_ARGUMENT && info.adapter_luid == 7, "short info checked before version without writes");
    info.struct_size++;
    check(rsf_srvk_fsr4_info(fake, &info) == RSF_BACKEND_ERROR_ABI_MISMATCH && !info.adapter_luid, "info ABI mismatch clears facts");
    info.abi_version = RSF_SRVK_FSR4_INFO_ABI_VERSION;
    uint32_t width = 9, height = 9;
    check(rsf_srvk_fsr4_plan(nullptr, &width, &height) == RSF_BACKEND_ERROR_INVALID_ARGUMENT && !width && !height, "invalid plan clears outputs");
    check(rsf_srvk_fsr4_destroy(nullptr) == 0, "null destroy");
    check(rsf_srvk_fsr4_submitted(nullptr, commands) == RSF_BACKEND_ERROR_INVALID_ARGUMENT, "notification owner required");
    check(rsf_srvk_fsr4_submitted(fake, nullptr) == RSF_BACKEND_ERROR_INVALID_ARGUMENT, "notification commands required");
    rsf_frame_record record{}; record.struct_size = sizeof(record); record.abi_version = RSF_GAME_FRAME_ABI_VERSION;
    record.camera.struct_size = sizeof(record.camera); record.camera.abi_version = RSF_GAME_CAMERA_ABI_VERSION;
    rsf_srvk_frame frame{}; frame.struct_size = sizeof(frame); frame.abi_version = RSF_SRVK_ABI_VERSION; frame.record = &record;
    void* continuation = nullptr;
    frame.struct_size = sizeof(frame) - 1; frame.abi_version = 99;
    check(rsf_srvk_fsr4_evaluate(fake, commands, &frame, &continuation) == RSF_BACKEND_ERROR_INVALID_ARGUMENT && continuation == commands, "short frame leaves original recording");
    frame.struct_size = sizeof(frame);
    check(rsf_srvk_fsr4_evaluate(fake, commands, &frame, &continuation) == RSF_BACKEND_ERROR_ABI_MISMATCH, "frame version");
    frame.abi_version = RSF_SRVK_ABI_VERSION;
    record.struct_size--;
    check(rsf_srvk_fsr4_evaluate(fake, commands, &frame, &continuation) == RSF_BACKEND_ERROR_INVALID_ARGUMENT, "short record");
    record.struct_size++; record.abi_version = 99;
    check(rsf_srvk_fsr4_evaluate(fake, commands, &frame, &continuation) == RSF_BACKEND_ERROR_ABI_MISMATCH, "record version");
    record.abi_version = RSF_GAME_FRAME_ABI_VERSION; record.camera.struct_size--;
    check(rsf_srvk_fsr4_evaluate(fake, commands, &frame, &continuation) == RSF_BACKEND_ERROR_INVALID_ARGUMENT, "short camera");
    record.camera.struct_size++; record.camera.abi_version = 99;
    check(rsf_srvk_fsr4_evaluate(fake, commands, &frame, &continuation) == RSF_BACKEND_ERROR_ABI_MISMATCH, "camera version");
    record.camera.abi_version = RSF_GAME_CAMERA_ABI_VERSION; frame.reactive.image = 1;
    check(rsf_srvk_fsr4_evaluate(fake, commands, &frame, &continuation) == RSF_BACKEND_ERROR_NOT_SUPPORTED, "unsupported masks not ignored");
    frame.reactive.image = 0;
    check(rsf_srvk_fsr4_evaluate(fake, commands, &frame, nullptr) == RSF_BACKEND_ERROR_INVALID_ARGUMENT, "continuation output required");
#if defined(RSF_TEST_SPLIT_DISABLED)
    check(rsf_srvk_fsr4_create(&setup, &handoff, &owner) == RSF_BACKEND_ERROR_NOT_COMPILED && !owner, "valid create explicitly not compiled");
    check(rsf_srvk_fsr4_plan(fake, &width, &height) == RSF_BACKEND_ERROR_NOT_COMPILED && !width && !height, "disabled plan");
    check(rsf_srvk_fsr4_evaluate(fake, commands, &frame, &continuation) == RSF_BACKEND_ERROR_NOT_COMPILED && continuation == commands, "disabled evaluation leaves original recording");
    check(rsf_srvk_fsr4_info(fake, &info) == RSF_BACKEND_ERROR_NOT_COMPILED && !info.provider_version, "disabled info is unknown");
    check(rsf_srvk_fsr4_destroy(fake) == RSF_BACKEND_ERROR_NOT_COMPILED, "disabled checked destroy");
    check(rsf_srvk_fsr4_submitted(fake, commands) == RSF_BACKEND_ERROR_NOT_COMPILED, "disabled notification is honest");
#elif defined(RSF_TEST_SPLIT_STUB)
    check(!creates && !evaluations, "invalid calls never reached provider");
    // Forward-compatible size extensions keep the original prefix and version.
    setup.struct_size += 8; handoff.struct_size += 8;
    check(rsf_srvk_fsr4_create(&setup, &handoff, &owner) == 0 && owner == &instance &&
        forwarded_setup == &setup && forwarded_handoff == &handoff, "compiled route forwards the actual setup and callback lease");
    handoff.abi_version = RSF_SRVK_SPLIT_TRACKED_ABI_VERSION;
    check(rsf_srvk_fsr4_create(&setup, &handoff, &owner) == 0, "tracked ABI routes to actual provider");
    check(rsf_srvk_fsr4_submitted(owner, commands) == 0, "notification reaches provider");
    check(rsf_srvk_fsr4_plan(owner, &width, &height) == 0 && width == 320 && height == 180, "provider plan");
    struct ExtendedInfo { rsf_srvk_fsr4_provider_info value; uint64_t tail; } extended{{sizeof(ExtendedInfo), 1}, 0xcafe};
    check(rsf_srvk_fsr4_info(owner, &extended.value) == 0 && extended.tail == 0xcafe &&
        extended.value.adapter_luid == 0x123456789abcdef0ull && extended.value.provider_version == 0x456789abcdef0123ull &&
        extended.value.vendor_id == 0x10de && extended.value.int8_compatibility == 1, "effective provider fields and appended canary");
    throw_info = true;
    check(rsf_srvk_fsr4_info(owner, &info) == RSF_BACKEND_ERROR_FEATURE_FAILED && !info.provider_version, "info exception preserves unknown facts");
    throw_info = false;
    frame.struct_size = uint32_t(offsetof(rsf_srvk_frame, bias_current_color));
    check(rsf_srvk_fsr4_evaluate(owner, commands, &frame, &continuation) == 0, "old ABI 2 frame prefix remains accepted");
    frame.struct_size = sizeof(frame);
    provider_result = -71;
    check(rsf_srvk_fsr4_info(owner, &info) == -71 && !info.adapter_luid && !info.provider_version && !info.vendor_id && !info.int8_compatibility,
        "failed private query never reports fabricated facts");
    check(rsf_srvk_fsr4_submitted(owner, commands) == -71, "notification preserves native errors");
    check(rsf_srvk_fsr4_evaluate(owner, commands, &frame, &continuation) == -71 && continuation == reinterpret_cast<void*>(2), "SDK failure preserves continuation and native error");
    check(rsf_srvk_fsr4_destroy(owner) == -71, "checked destroy failure preserves owner");
    owner = nullptr;
    check(rsf_srvk_fsr4_create(&setup, &handoff, &owner) == -71 && owner == &instance, "failed creation preserves cleanup lease");
    throw_evaluate = true;
    check(rsf_srvk_fsr4_evaluate(owner, commands, &frame, &continuation) == RSF_BACKEND_ERROR_FEATURE_FAILED &&
        continuation == reinterpret_cast<void*>(2), "exception cannot escape or lose a successfully leased continuation");
    provider_result = 0;
    check(rsf_srvk_fsr4_destroy(owner) == 0, "destroy retry");
#else
    // Production DLL implementation: absent dispatch is rejected before touching
    // fake handles; never run a valid evaluate with a fake opaque owner.
    const auto result = rsf_srvk_fsr4_create(&setup, &handoff, &owner);
    check(result != 0 && !owner, "production create refuses unavailable capabilities");
#endif
    std::puts(failures ? "split ABI FAILED" : "split ABI passed (contract only, no GPU)");
    return failures ? 1 : 0;
}

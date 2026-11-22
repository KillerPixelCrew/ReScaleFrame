// CPU-only fixture: the real XeSS leaves and Rust owner with inert SDK and Vulkan entry points.
#if defined(RSF_TEST_XESS_INPUT_RANGE)
#include "../runtime/orchestrator/src/native_xess_vulkan.cpp"
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <initializer_list>
// The CPU contract must execute its checks in Release as well as Debug.
#define CHECK(condition) do { if (!(condition)) { \
    std::fprintf(stderr, "XeSS input range contract failed: %s\n", #condition); std::abort(); \
} } while (0)
namespace {
unsigned executed{}, copied{}, images{}, destroyed_images{}, views{}, destroyed_views{}, freed{}, contexts{}, destroyed_contexts{};
uint32_t expected_width{}, expected_height{}, expected_reset{};
xess_result_t create(VkInstance, VkPhysicalDevice, VkDevice device, xess_context_handle_t* out) {
    CHECK(device == reinterpret_cast<VkDevice>(1));
    *out = reinterpret_cast<xess_context_handle_t>(0x40); ++contexts; return XESS_RESULT_SUCCESS;
}
xess_result_t initialize(xess_context_handle_t, const xess_vk_init_params_t* p) {
    CHECK(p->outputResolution.x == 1920 && p->outputResolution.y == 1080);
    CHECK(p->qualitySetting == XESS_QUALITY_SETTING_QUALITY && p->initFlags == XESS_INIT_FLAG_ENABLE_AUTOEXPOSURE);
    return XESS_RESULT_SUCCESS;
}
xess_result_t resolution(xess_context_handle_t, const xess_2d_t* output, xess_quality_settings_t quality,
    xess_2d_t* recommended, xess_2d_t* minimum, xess_2d_t* maximum) {
    CHECK(output->x == 1920 && output->y == 1080 && quality == XESS_QUALITY_SETTING_QUALITY);
    *recommended = {1130, 636}; *minimum = {1024, 576}; *maximum = {1280, 720};
    return XESS_RESULT_SUCCESS;
}
xess_result_t velocity(xess_context_handle_t, float x, float y) { CHECK(x == 1 && y == 1); return XESS_RESULT_SUCCESS; }
xess_result_t execute(xess_context_handle_t, VkCommandBuffer, const xess_vk_execute_params_t* p) {
    CHECK(p->inputWidth == expected_width && p->inputHeight == expected_height && p->resetHistory == expected_reset);
    // Private allocations cover the SDK maximum. Active input is a genuine
    // subrectangle, not a fabricated VkImage allocation extent.
    CHECK(p->colorTexture.width == 1280 && p->colorTexture.height == 720);
    CHECK(p->depthTexture.width == 1280 && p->velocityTexture.height == 720);
    CHECK(p->outputTexture.width == 1920 && p->outputTexture.height == 1080);
    CHECK(p->colorTexture.imageView && p->outputTexture.imageView && p->exposureScale == 1);
    for (const auto* texture : {&p->colorTexture, &p->depthTexture, &p->velocityTexture, &p->outputTexture}) {
        const auto& range = texture->subresourceRange;
        CHECK(range.aspectMask == VK_IMAGE_ASPECT_COLOR_BIT && range.baseMipLevel == 0 &&
            range.levelCount == 1 && range.baseArrayLayer == 0 && range.layerCount == 1);
    }
    CHECK(!p->exposureScaleTexture.image && !p->responsivePixelMaskTexture.image);
    CHECK(p->inputColorBase.x == 0 && p->inputColorBase.y == 0 &&
        p->inputDepthBase.x == 0 && p->inputDepthBase.y == 0 &&
        p->inputMotionVectorBase.x == 0 && p->inputMotionVectorBase.y == 0 &&
        p->outputColorBase.x == 0 && p->outputColorBase.y == 0);
    ++executed; return XESS_RESULT_SUCCESS;
}
xess_result_t destroy(xess_context_handle_t context) {
    CHECK(context == reinterpret_cast<xess_context_handle_t>(0x40)); ++destroyed_contexts; return XESS_RESULT_SUCCESS;
}
xess_result_t version(xess_version_t* value) { *value = {2, 1, 3, 4}; return XESS_RESULT_SUCCESS; }
xess_result_t instance_extensions(uint32_t*, const char* const**, uint32_t*) { return XESS_RESULT_SUCCESS; }
xess_result_t device_extensions(VkInstance, VkPhysicalDevice, uint32_t*, const char* const**) { return XESS_RESULT_SUCCESS; }
xess_result_t device_features(VkInstance, VkPhysicalDevice, void**) { return XESS_RESULT_SUCCESS; }
unsigned sdk_lookups{}, sdk_pins{};
bool omit_create = true;
void* sdk_lookup(void* module, const char* name) {
    const struct { const char* name; void* address; } exports[] = {
        {"xessVKGetRequiredInstanceExtensions", reinterpret_cast<void*>(instance_extensions)},
        {"xessVKGetRequiredDeviceExtensions", reinterpret_cast<void*>(device_extensions)},
        {"xessVKGetRequiredDeviceFeatures", reinterpret_cast<void*>(device_features)},
        {"xessVKCreateContext", reinterpret_cast<void*>(create)},
        {"xessVKInit", reinterpret_cast<void*>(initialize)},
        {"xessVKExecute", reinterpret_cast<void*>(execute)},
        {"xessDestroyContext", reinterpret_cast<void*>(destroy)},
        {"xessGetOptimalInputResolution", reinterpret_cast<void*>(resolution)},
        {"xessSetVelocityScale", reinterpret_cast<void*>(velocity)},
        {"xessGetVersion", reinterpret_cast<void*>(version)},
    };
    CHECK(module == &sdk_lookups && sdk_lookups < sizeof(exports) / sizeof(exports[0]));
    const auto& expected = exports[sdk_lookups++];
    CHECK(std::strcmp(name, expected.name) == 0);
    return omit_create && sdk_lookups == 4 ? nullptr : expected.address;
}
VKAPI_ATTR VkResult VKAPI_CALL create_image(VkDevice, const VkImageCreateInfo* info, const VkAllocationCallbacks*, VkImage* out) {
    CHECK((info->extent.width == 1280 && info->extent.height == 720 && info->usage == 6) ||
        (info->extent.width == 1920 && info->extent.height == 1080 && info->usage == 15));
    *out = reinterpret_cast<VkImage>(uintptr_t(0x100 + ++images)); return VK_SUCCESS;
}
VKAPI_ATTR void VKAPI_CALL destroy_image(VkDevice, VkImage, const VkAllocationCallbacks*) { ++destroyed_images; }
VKAPI_ATTR void VKAPI_CALL requirements(VkDevice, VkImage, VkMemoryRequirements* out) { *out = {}; out->size = 1; out->memoryTypeBits = 1; }
VKAPI_ATTR VkResult VKAPI_CALL allocate_memory(VkDevice, const VkMemoryAllocateInfo* info, const VkAllocationCallbacks*, VkDeviceMemory* out) {
    CHECK(info->memoryTypeIndex == 0); *out = reinterpret_cast<VkDeviceMemory>(uintptr_t(0x200 + images)); return VK_SUCCESS;
}
VKAPI_ATTR void VKAPI_CALL free_memory(VkDevice, VkDeviceMemory, const VkAllocationCallbacks*) { ++freed; }
VKAPI_ATTR VkResult VKAPI_CALL bind(VkDevice, VkImage, VkDeviceMemory, VkDeviceSize) { return VK_SUCCESS; }
VKAPI_ATTR VkResult VKAPI_CALL create_view(VkDevice, const VkImageViewCreateInfo*, const VkAllocationCallbacks*, VkImageView* out) {
    *out = reinterpret_cast<VkImageView>(uintptr_t(0x300 + ++views)); return VK_SUCCESS;
}
VKAPI_ATTR void VKAPI_CALL destroy_view(VkDevice, VkImageView, const VkAllocationCallbacks*) { ++destroyed_views; }
VKAPI_ATTR void VKAPI_CALL barrier(VkCommandBuffer, VkPipelineStageFlags, VkPipelineStageFlags, VkDependencyFlags,
    uint32_t, const VkMemoryBarrier*, uint32_t, const VkBufferMemoryBarrier*, uint32_t, const VkImageMemoryBarrier*) {}
VKAPI_ATTR void VKAPI_CALL copy(VkCommandBuffer, VkImage, VkImageLayout, VkImage, VkImageLayout, uint32_t, const VkImageCopy* p) {
    CHECK((p->extent.width == expected_width && p->extent.height == expected_height) ||
        (p->extent.width == 1920 && p->extent.height == 1080)); ++copied;
}
VKAPI_ATTR void VKAPI_CALL memory_properties(VkPhysicalDevice, VkPhysicalDeviceMemoryProperties* out) {
    *out = {}; out->memoryTypeCount = 1; out->memoryTypes[0].propertyFlags = VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT;
}
template<class T> rsf_vk_function entry(T function) { return reinterpret_cast<rsf_vk_function>(function); }
rsf_vk_function device_proc(void*, const char* name) {
    const struct { const char* name; rsf_vk_function function; } table[] = {
        {"vkCreateImage", entry(create_image)}, {"vkDestroyImage", entry(destroy_image)},
        {"vkGetImageMemoryRequirements", entry(requirements)}, {"vkAllocateMemory", entry(allocate_memory)},
        {"vkFreeMemory", entry(free_memory)}, {"vkBindImageMemory", entry(bind)},
        {"vkCreateImageView", entry(create_view)}, {"vkDestroyImageView", entry(destroy_view)},
        {"vkCmdPipelineBarrier", entry(barrier)}, {"vkCmdCopyImage", entry(copy)}};
    for (const auto& value : table) if (!std::strcmp(value.name, name)) return value.function;
    return nullptr; // Capture stays disabled without its buffer entry points.
}
rsf_vk_function instance_proc(void*, const char* name) {
    return std::strcmp(name, "vkGetPhysicalDeviceMemoryProperties") ? nullptr : entry(memory_properties);
}
}
int main() {
    auto ops = rsf::xess_vk::native_ops;
    ops.loader = {[](const uint16_t*) -> void* { return &sdk_lookups; }, [](void*) {}};
    ops.lookup = sdk_lookup;
    ops.pin = [](void*) -> uint32_t { ++sdk_pins; return 1; };
    const rsf_xessvk_setup session{sizeof(session), RSF_XESSVK_REQUIREMENTS_ABI_VERSION, "C:\\fake"};
    CHECK(rsf_core_xessvk_initialize(&ops, &session) == RSF_BACKEND_ERROR_MISSING_ENTRY_POINT);
    CHECK(sdk_lookups == 4 && sdk_pins == 0);
    omit_create = false;
    sdk_lookups = 0;
    CHECK(rsf_core_xessvk_initialize(&ops, &session) == RSF_BACKEND_OK);
    CHECK(sdk_lookups == 10 && sdk_pins == 1);
    void* const device = reinterpret_cast<void*>(1);
    rsf_srvk_setup setup{sizeof(setup), RSF_SRVK_ABI_VERSION, device, device, device, instance_proc, device_proc,
        RSF_SR_XESS, 1, 1920, 1080, 0, 0, 0, "C:\\fake"};
    rsf_srvk* self = nullptr;
    CHECK(rsf_srvk_create(&setup, &self) == RSF_BACKEND_OK && self);
    CHECK(contexts == 1 && images == 4 && views == 4);
    CHECK(rsf_core_xessvk_shutdown(&ops) == RSF_BACKEND_ERROR_NOT_READY);
    rsf_frame_record record{}; record.struct_size = sizeof(record); record.abi_version = RSF_GAME_FRAME_ABI_VERSION;
    record.camera.struct_size = sizeof(record.camera); record.camera.abi_version = RSF_GAME_CAMERA_ABI_VERSION;
    record.session_id = record.view_id = record.resource_generation = 1; record.frame_time_ms = 16;
    record.camera.near_plane = .1f; record.camera.far_plane = 100; record.camera.vertical_fov_radians = 1;
    record.output_width = record.camera.output_width = 1920; record.output_height = record.camera.output_height = 1080;
    rsf_srvk_frame input{}; input.struct_size = sizeof(input); input.abi_version = RSF_SRVK_ABI_VERSION; input.record = &record;
    input.pre_exposure = input.motion_scale_x = input.motion_scale_y = 1;
    input.sharpening.struct_size = sizeof(input.sharpening); input.sharpening.abi_version = RSF_SHARPENING_ABI_VERSION;
    input.sharpening.mode = RSF_SHARPENING_OFF;
    auto dimensions = [&](uint32_t w, uint32_t h) {
        record.render_width = record.camera.render_width = w; record.render_height = record.camera.render_height = h;
        input.color = {device, 20, VK_FORMAT_R16G16B16A16_SFLOAT, w, h, VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL, 1};
        input.depth = {device, 21, VK_FORMAT_R32_SFLOAT, w, h, VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL, 1};
        input.motion = {device, 22, VK_FORMAT_R16G16_SFLOAT, w, h, VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL, 1};
        input.output = {device, 23, VK_FORMAT_R16G16B16A16_SFLOAT, 1920, 1080, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, 1};
        expected_width = w; expected_height = h;
    };
    void* const commands = reinterpret_cast<void*>(2);
    uint32_t w{}, h{}; CHECK(rsf_srvk_plan(self, &w, &h) == 0 && w == 1130 && h == 636);
    dimensions(1128, 632); record.frame_id = 1; expected_reset = 1;
    CHECK(rsf_srvk_evaluate(self, commands, &input) == 0 && executed == 1 && copied == 4);
    record.frame_id = 2; expected_reset = 0;
    CHECK(rsf_srvk_evaluate(self, commands, &input) == 0 && executed == 2);
    dimensions(1280, 720); record.frame_id = 3; expected_reset = 1;
    CHECK(rsf_srvk_evaluate(self, commands, &input) == 0 && executed == 3);
    auto reject = [&] {
        const unsigned before = copied;
        CHECK(rsf_srvk_evaluate(self, commands, &input) == RSF_BACKEND_ERROR_INVALID_ARGUMENT && copied == before && executed == 3);
    };
    dimensions(1023, 632); reject(); dimensions(1128, 575); reject();
    dimensions(1281, 632); reject(); dimensions(1128, 721); reject();
    dimensions(0, 632); reject(); dimensions(1128, 632); input.depth.width--; reject();
    dimensions(1128, 632); input.motion.height--; reject();
    dimensions(1128, 632); record.camera.render_width--; reject();
    dimensions(1128, 632); record.camera.output_height--; reject();
    record.camera.output_height++;
    rsf_srvk_retire_commands(self, commands);
    CHECK(rsf_srvk_destroy(self) == RSF_BACKEND_OK);
    CHECK(destroyed_contexts == 1 && destroyed_images == 4 && destroyed_views == 4 && freed == 4);
    CHECK(rsf_core_xessvk_shutdown(&ops) == RSF_BACKEND_OK);
    std::puts("PASS: XeSS Vulkan input range, cache sizing, reset and teardown");
    return 0;
}
#endif // RSF_TEST_XESS_INPUT_RANGE

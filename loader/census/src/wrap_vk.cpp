// SPDX-License-Identifier: GPL-3.0-only
// Vulkan: instance and device creation, queues, submits, swapchain creation and presentation.
//
// A game reaches these through vkGetInstanceProcAddr and vkGetDeviceProcAddr as often as through
// the loader's own exports, so both lookups hand out recording thunks for the names below. A name
// that resolves to two different implementations (for example a device-level pointer after an
// instance-level one) keeps the first and counts a conflict; see the README.
//
// Structure offsets are written out from the Vulkan 1.3 specification (LP64 layout) because the
// Vulkan headers are not required to build the census.
#include "wrap_generic.h"

namespace rsf_census {

namespace {

bool is_vulkan_fn(Fn f) noexcept { return f != Fn::Count && !std::strcmp(kFns[idx(f)].api, "vulkan"); }

Q proc_addr(Fn self, Q handle, Q name, void* return_address) noexcept {
    using Target = Q (*)(Q, Q);
    auto o = reinterpret_cast<Target>(orig_by(self));
    Call c(self, return_address);
    char nm[96]{};
    const bool have = read_arg_string(reinterpret_cast<void*>(name), nm, sizeof(nm));
    Q r = o ? o(handle, name) : 0;
    if (c.full()) {
        c.json().hex("handle", handle);
        c.json().str("name", nm);
        c.json().hex("result", r);
    }
    c.finish(r);
    if (r && have) {
        const Fn f = lookup_name(nm);
        if (is_vulkan_fn(f)) r = reinterpret_cast<Q>(bind_thunk(f, reinterpret_cast<void*>(r)));
    }
    return r;
}

Q vk_gipa(Q instance, Q name) noexcept {
    return proc_addr(Fn::VkGetInstanceProcAddr, instance, name, _ReturnAddress());
}
Q vk_gdpa(Q device, Q name) noexcept {
    return proc_addr(Fn::VkGetDeviceProcAddr, device, name, _ReturnAddress());
}

void put_name_list(Json& j, const char* key, const void* names, uint32_t count) noexcept {
    j.arr(key);
    for (uint32_t i = 0; i < count && i < 96; ++i) {
        const char* s = nullptr;
        char buf[96];
        if (!plausible_ptr(names) || !rd(static_cast<const char* const*>(names) + i, s) ||
            !read_arg_string(s, buf, sizeof(buf))) {
            break;
        }
        j.str(nullptr, buf);
    }
    j.end_arr();
}

void dec_create_instance(Json& j, const Q* a) noexcept {
    uint8_t ci[64];
    if (!plausible_ptr(reinterpret_cast<void*>(a[0])) || !safe_read(reinterpret_cast<void*>(a[0]), ci, sizeof(ci))) {
        j.boolean("create_info_unreadable", true);
        return;
    }
    // VkInstanceCreateInfo: pApplicationInfo@24, enabledLayerCount@32, ppEnabledLayerNames@40,
    // enabledExtensionCount@48, ppEnabledExtensionNames@56.
    void* app;
    uint32_t nl, ne;
    void *layers, *exts;
    std::memcpy(&app, ci + 24, 8);
    std::memcpy(&nl, ci + 32, 4);
    std::memcpy(&layers, ci + 40, 8);
    std::memcpy(&ne, ci + 48, 4);
    std::memcpy(&exts, ci + 56, 8);
    uint8_t ai[48];
    if (plausible_ptr(app) && safe_read(app, ai, sizeof(ai))) {
        // VkApplicationInfo: pApplicationName@16, applicationVersion@24, pEngineName@32,
        // engineVersion@40, apiVersion@44.
        const char *an, *en;
        uint32_t av, ev, api;
        std::memcpy(&an, ai + 16, 8);
        std::memcpy(&av, ai + 24, 4);
        std::memcpy(&en, ai + 32, 8);
        std::memcpy(&ev, ai + 40, 4);
        std::memcpy(&api, ai + 44, 4);
        char s[96];
        if (read_arg_string(an, s, sizeof(s))) j.str("application", s);
        if (read_arg_string(en, s, sizeof(s))) j.str("engine", s);
        j.hex("application_version", av);
        j.hex("engine_version", ev);
        j.u64("api_major", (api >> 22) & 0x7F);
        j.u64("api_minor", (api >> 12) & 0x3FF);
    }
    put_name_list(j, "layers", layers, nl);
    put_name_list(j, "extensions", exts, ne);
}
void post_create_instance(Json& j, const Q* a, Q) noexcept { put_out_ptr(j, "instance", a[2]); }

void dec_create_device(Json& j, const Q* a) noexcept {
    j.hex("physical_device", a[0]);
    uint8_t ci[72];
    if (!plausible_ptr(reinterpret_cast<void*>(a[1])) || !safe_read(reinterpret_cast<void*>(a[1]), ci, sizeof(ci))) {
        j.boolean("create_info_unreadable", true);
        return;
    }
    // VkDeviceCreateInfo: queueCreateInfoCount@20, pQueueCreateInfos@24, enabledLayerCount@32,
    // ppEnabledLayerNames@40, enabledExtensionCount@48, ppEnabledExtensionNames@56.
    uint32_t nq, nl, ne;
    void *queues, *layers, *exts;
    std::memcpy(&nq, ci + 20, 4);
    std::memcpy(&queues, ci + 24, 8);
    std::memcpy(&nl, ci + 32, 4);
    std::memcpy(&layers, ci + 40, 8);
    std::memcpy(&ne, ci + 48, 4);
    std::memcpy(&exts, ci + 56, 8);
    j.arr("queue_families");
    for (uint32_t i = 0; i < nq && i < 16; ++i) {
        // VkDeviceQueueCreateInfo (40 bytes): queueFamilyIndex@20, queueCount@24.
        uint8_t q[40];
        if (!plausible_ptr(queues) || !safe_read(static_cast<const uint8_t*>(queues) + i * 40, q, sizeof(q))) break;
        uint32_t family, count;
        std::memcpy(&family, q + 20, 4);
        std::memcpy(&count, q + 24, 4);
        j.obj();
        j.u64("family", family);
        j.u64("count", count);
        j.end_obj();
    }
    j.end_arr();
    put_name_list(j, "layers", layers, nl);
    put_name_list(j, "extensions", exts, ne);
}
void post_create_device(Json& j, const Q* a, Q) noexcept { put_out_ptr(j, "device", a[3]); }

void dec_get_queue(Json& j, const Q* a) noexcept {
    j.hex("device", a[0]);
    j.u64("family", static_cast<uint32_t>(a[1]));
    j.u64("index", static_cast<uint32_t>(a[2]));
}
void post_get_queue(Json& j, const Q* a, Q) noexcept { put_out_ptr(j, "queue", a[3]); }
void act_get_queue(const Q* a, Q) noexcept {
    void* q = nullptr;
    if (plausible_ptr(reinterpret_cast<void*>(a[3])) && rd(reinterpret_cast<void*>(a[3]), q) && q) {
        queue_describe(QueueKind::Vulkan, q, static_cast<uint32_t>(a[1]), static_cast<uint32_t>(a[2]), 0);
    }
}
void dec_get_queue2(Json& j, const Q* a) noexcept {
    j.hex("device", a[0]);
    uint32_t info[8]{};
    if (plausible_ptr(reinterpret_cast<void*>(a[1])) && safe_read(reinterpret_cast<void*>(a[1]), info, sizeof(info))) {
        // VkDeviceQueueInfo2: flags@16, queueFamilyIndex@20, queueIndex@24.
        j.u64("family", info[5]);
        j.u64("index", info[6]);
    }
}
void post_get_queue2(Json& j, const Q* a, Q) noexcept { put_out_ptr(j, "queue", a[2]); }
void act_get_queue2(const Q* a, Q) noexcept {
    void* q = nullptr;
    uint32_t info[8]{};
    if (plausible_ptr(reinterpret_cast<void*>(a[2])) && rd(reinterpret_cast<void*>(a[2]), q) && q &&
        plausible_ptr(reinterpret_cast<void*>(a[1])) && safe_read(reinterpret_cast<void*>(a[1]), info, sizeof(info))) {
        queue_describe(QueueKind::Vulkan, q, info[5], info[6], 0);
    }
}

void dec_submit(Json& j, const Q* a) noexcept {
    j.hex("queue", a[0]);
    const uint32_t n = static_cast<uint32_t>(a[1]);
    j.u64("batches", n);
    j.hex("fence", a[3]);
    uint32_t cmds = 0, waits = 0, signals = 0;
    for (uint32_t i = 0; i < n && i < 16; ++i) {
        // VkSubmitInfo (72 bytes): waitSemaphoreCount@16, commandBufferCount@40, signalSemaphoreCount@56.
        uint8_t s[72];
        if (!plausible_ptr(reinterpret_cast<void*>(a[2])) ||
            !safe_read(reinterpret_cast<uint8_t*>(a[2]) + i * 72, s, sizeof(s))) {
            break;
        }
        uint32_t w, c, sg;
        std::memcpy(&w, s + 16, 4);
        std::memcpy(&c, s + 40, 4);
        std::memcpy(&sg, s + 56, 4);
        waits += w;
        cmds += c;
        signals += sg;
    }
    j.u64("command_buffers_in_first_16", cmds);
    j.u64("wait_semaphores", waits);
    j.u64("signal_semaphores", signals);
}
void act_submit(const Q* a, Q) noexcept { queue_submit(QueueKind::Vulkan, reinterpret_cast<void*>(a[0]), static_cast<uint32_t>(a[1])); }

void dec_submit2(Json& j, const Q* a) noexcept {
    j.hex("queue", a[0]);
    const uint32_t n = static_cast<uint32_t>(a[1]);
    j.u64("batches", n);
    j.hex("fence", a[3]);
    uint32_t cmds = 0, waits = 0, signals = 0;
    for (uint32_t i = 0; i < n && i < 16; ++i) {
        // VkSubmitInfo2 (64 bytes): waitSemaphoreInfoCount@20, commandBufferInfoCount@32,
        // signalSemaphoreInfoCount@48.
        uint8_t s[64];
        if (!plausible_ptr(reinterpret_cast<void*>(a[2])) ||
            !safe_read(reinterpret_cast<uint8_t*>(a[2]) + i * 64, s, sizeof(s))) {
            break;
        }
        uint32_t w, c, sg;
        std::memcpy(&w, s + 20, 4);
        std::memcpy(&c, s + 32, 4);
        std::memcpy(&sg, s + 48, 4);
        waits += w;
        cmds += c;
        signals += sg;
    }
    j.u64("command_buffers_in_first_16", cmds);
    j.u64("wait_semaphores", waits);
    j.u64("signal_semaphores", signals);
}

void dec_create_swapchain(Json& j, const Q* a) noexcept {
    j.hex("device", a[0]);
    uint8_t ci[104];
    if (!plausible_ptr(reinterpret_cast<void*>(a[1])) || !safe_read(reinterpret_cast<void*>(a[1]), ci, sizeof(ci))) {
        j.boolean("create_info_unreadable", true);
        return;
    }
    // VkSwapchainCreateInfoKHR: flags@16, surface@24, minImageCount@32, imageFormat@36,
    // imageColorSpace@40, imageExtent@44, imageArrayLayers@52, imageUsage@56, imageSharingMode@60,
    // preTransform@80, compositeAlpha@84, presentMode@88, clipped@92, oldSwapchain@96.
    uint32_t min_images, format, color_space, w, h, layers, usage, sharing, pre, alpha, mode, clipped;
    uint64_t surface, old;
    std::memcpy(&surface, ci + 24, 8);
    std::memcpy(&min_images, ci + 32, 4);
    std::memcpy(&format, ci + 36, 4);
    std::memcpy(&color_space, ci + 40, 4);
    std::memcpy(&w, ci + 44, 4);
    std::memcpy(&h, ci + 48, 4);
    std::memcpy(&layers, ci + 52, 4);
    std::memcpy(&usage, ci + 56, 4);
    std::memcpy(&sharing, ci + 60, 4);
    std::memcpy(&pre, ci + 80, 4);
    std::memcpy(&alpha, ci + 84, 4);
    std::memcpy(&mode, ci + 88, 4);
    std::memcpy(&clipped, ci + 92, 4);
    std::memcpy(&old, ci + 96, 8);
    static const char* const kModes[] = {"IMMEDIATE", "MAILBOX", "FIFO", "FIFO_RELAXED"};
    j.hex("surface", surface);
    j.u64("min_image_count", min_images);
    j.u64("image_format", format);
    j.u64("image_color_space", color_space);
    j.u64("width", w);
    j.u64("height", h);
    j.u64("array_layers", layers);
    j.hex("image_usage", usage);
    j.u64("sharing_mode", sharing);
    j.hex("pre_transform", pre);
    j.hex("composite_alpha", alpha);
    j.u64("present_mode", mode);
    j.str("present_mode_name", mode < 4 ? kModes[mode] : "other");
    j.boolean("clipped", clipped != 0);
    j.hex("old_swapchain", old);
}
void post_create_swapchain(Json& j, const Q* a, Q) noexcept {
    uint64_t sc;
    if (plausible_ptr(reinterpret_cast<void*>(a[3])) && rd(reinterpret_cast<void*>(a[3]), sc)) j.hex("swapchain", sc);
}

Q queue_present(Q queue, Q info) noexcept {
    using Target = Q (*)(Q, Q);
    auto o = reinterpret_cast<Target>(orig<Fn::VkQueuePresentKHR>());
    PresentScope scope;
    if (scope.outer) note_present_begin();
    Call c(Fn::VkQueuePresentKHR, _ReturnAddress());
    if (c.full()) {
        c.json().hex("queue", queue);
        uint8_t p[64];
        if (plausible_ptr(reinterpret_cast<void*>(info)) && safe_read(reinterpret_cast<void*>(info), p, sizeof(p))) {
            // VkPresentInfoKHR: waitSemaphoreCount@16, swapchainCount@32, pSwapchains@40, pImageIndices@48.
            uint32_t waits, count;
            void *chains, *indices;
            std::memcpy(&waits, p + 16, 4);
            std::memcpy(&count, p + 32, 4);
            std::memcpy(&chains, p + 40, 8);
            std::memcpy(&indices, p + 48, 8);
            c.json().u64("wait_semaphores", waits);
            c.json().u64("swapchain_count", count);
            c.json().arr("swapchains");
            for (uint32_t i = 0; i < count && i < 4; ++i) {
                uint64_t sc;
                if (!plausible_ptr(chains) || !rd(static_cast<const uint64_t*>(chains) + i, sc)) break;
                c.json().hex(nullptr, sc);
            }
            c.json().end_arr();
            c.json().arr("image_indices");
            for (uint32_t i = 0; i < count && i < 4; ++i) {
                uint32_t ix;
                if (!plausible_ptr(indices) || !rd(static_cast<const uint32_t*>(indices) + i, ix)) break;
                c.json().u64(nullptr, ix);
            }
            c.json().end_arr();
        }
    }
    const Q r = o ? o(queue, info) : 0;
    c.finish(r);
    return r;
}

} // namespace

void register_wrappers_vk(void** t) noexcept {
    t[idx(Fn::VkGetInstanceProcAddr)] = reinterpret_cast<void*>(&vk_gipa);
    t[idx(Fn::VkGetDeviceProcAddr)] = reinterpret_cast<void*>(&vk_gdpa);
    RSF_REG(t, VkCreateInstance, dec_create_instance, post_create_instance);
    RSF_REG(t, VkCreateDevice, dec_create_device, post_create_device);
    RSF_REG(t, VkGetDeviceQueue, dec_get_queue, post_get_queue, act_get_queue);
    RSF_REG(t, VkGetDeviceQueue2, dec_get_queue2, post_get_queue2, act_get_queue2);
    RSF_REG(t, VkQueueSubmit, dec_submit, nullptr, act_submit);
    RSF_REG(t, VkQueueSubmit2, dec_submit2, nullptr, act_submit);
    RSF_REG(t, VkQueueSubmit2KHR, dec_submit2, nullptr, act_submit);
    t[idx(Fn::VkQueuePresentKHR)] = reinterpret_cast<void*>(&queue_present);
    RSF_REG(t, VkCreateSwapchainKHR, dec_create_swapchain, post_create_swapchain);
}

} // namespace rsf_census

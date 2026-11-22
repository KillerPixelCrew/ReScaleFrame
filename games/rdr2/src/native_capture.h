// SPDX-License-Identifier: MIT
#pragma once
#include <cstddef>
#include <cstdint>

extern "C" bool rsf_rdr2_install_native_capture(const wchar_t* directory) noexcept;

#ifdef RSF_RDR2_CAPTURE_TEST
void rsf_rdr2_capture_test_arm(void* swapchain) noexcept;
void rsf_rdr2_capture_test_stop() noexcept;
#endif

struct rsf_rdr2_capture_state;

struct rsf_rdr2_capture_paths {
    wchar_t directory[260], request[260], log[260];
};

// Native effects used by the Rust installation owner. The leaf table must remain live until exit.
struct rsf_rdr2_capture_install_ops {
    uint32_t struct_size;
    uint32_t version;
    bool (*create_state)(rsf_rdr2_capture_state*, const rsf_rdr2_capture_paths*) noexcept;
    void (*discard_state)() noexcept;
    void* (*start_worker)() noexcept;
    void (*observe_d3d12)() noexcept;
    void (*observe_ngx)() noexcept;
    void (*wait_worker)(void* thread, uint32_t timeout_ms) noexcept;
    void (*close_worker)(void* thread) noexcept;
    void (*report)(const char* message) noexcept;
};
static_assert(sizeof(wchar_t) == 2 && sizeof(rsf_rdr2_capture_paths) == 3 * 260 * 2);
static_assert(sizeof(rsf_rdr2_capture_install_ops) == 8 + 8 * sizeof(void*));
static_assert(offsetof(rsf_rdr2_capture_install_ops, observe_d3d12) == 8 + 3 * sizeof(void*));
static_assert(offsetof(rsf_rdr2_capture_install_ops, wait_worker) == 8 + 5 * sizeof(void*));

extern "C" {
bool rsf_rdr2_capture_install(const wchar_t* directory, const rsf_rdr2_capture_install_ops*, bool fixture) noexcept;
void rsf_rdr2_capture_stop_fixture() noexcept;
}

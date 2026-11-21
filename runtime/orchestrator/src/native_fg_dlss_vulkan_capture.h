// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/native_fg_dlss_vulkan.h>

struct rsf_dlss_capture;

namespace rsf {
// Native handle of the Rust dedicated present capture owner.
class DlssPresentCapture {
    rsf_dlss_capture* owner = nullptr;
public:
    DlssPresentCapture() = default;
    ~DlssPresentCapture();
    DlssPresentCapture(const DlssPresentCapture&) = delete;
    DlssPresentCapture& operator=(const DlssPresentCapture&) = delete;
    rsf_backend_result initialize(const rsf_fgvk_setup&);
    void remember(const rsf_frame_record&, uint32_t generated_count, const rsf_dlssgvk_status&, bool configured);
    /* Presenter and GPU must already be drained. Detach keeps state alive until owner deletion. */
    void flush();
    rsf_backend_result detach();
};
}

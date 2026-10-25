// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/native_fg_vulkan.h>
#include <memory>

namespace rsf {
class DlssPresentCapture {
    struct State;
    std::unique_ptr<State> state;
public:
    DlssPresentCapture();
    ~DlssPresentCapture();
    DlssPresentCapture(const DlssPresentCapture&) = delete;
    DlssPresentCapture& operator=(const DlssPresentCapture&) = delete;
    rsf_backend_result initialize(const rsf_fgvk_setup&);
    void remember(const rsf_frame_record&, uint32_t generated_count);
    /* Presenter and GPU must already be drained before these operations. */
    void flush();
    rsf_backend_result shutdown();
};
}

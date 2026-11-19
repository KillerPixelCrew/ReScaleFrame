// SPDX-License-Identifier: MIT
#pragma once
#include <stdint.h>
namespace rsf::sr {
struct SplitLease;
extern "C" {
int rsf_core_split_pending(const SplitLease*);
void rsf_core_split_begin(SplitLease*, void*);
void rsf_core_split_resume(SplitLease*, void*);
int rsf_core_split_submitted(SplitLease*, void*);
}

// CPU admission only. Successful submission never substitutes for GPU completion.
struct SplitLease {
    uint32_t tracked = 0;
    void* producer = nullptr;
    void* continuation = nullptr;
    bool pending() const { return rsf_core_split_pending(this) != 0; }
    void begin(void* commands) { rsf_core_split_begin(this, commands); }
    void resume(void* commands) { rsf_core_split_resume(this, commands); }
    bool submitted(void* commands) { return rsf_core_split_submitted(this, commands) != 0; }
};
static_assert(sizeof(SplitLease) == 24);
}

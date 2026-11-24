// SPDX-License-Identifier: MIT
// The guarded engine-memory copy for the Rust owners; the hook set is native_hooks.rs.
#include "native_hooks.h"
#include "native_memory.h"

_Static_assert(sizeof(rsf_anomaly_hook_set) == 8, "the Rust owner holds the set as one pointer");

bool rsf_anomaly_guarded_copy(void* output, const void* input, size_t size)
{
    return guarded_copy(output, input, size);
}

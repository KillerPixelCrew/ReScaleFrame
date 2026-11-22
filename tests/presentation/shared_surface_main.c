/* SPDX-License-Identifier: MIT */
#include <stddef.h>
#include <rescaleframe/shared_surface.h>

_Static_assert(sizeof(rsf_shared_surface_setup) == 48 &&
    offsetof(rsf_shared_surface_setup, log) == 24 &&
    offsetof(rsf_shared_surface_setup, unordered_access) == 40, "shared surface layout");
_Static_assert(RSF_SHARED_SURFACE_ABI_VERSION == 2 && RSF_SHARED_OK == 0 &&
    RSF_SHARED_ERROR_INVALID_ARGUMENT == -1 && RSF_SHARED_ERROR_ABI_MISMATCH == -2 &&
    RSF_SHARED_ERROR_CREATE_FAILED == -3 && RSF_SHARED_ERROR_NO_HANDLE == -4 &&
    RSF_SHARED_ERROR_OPEN_FAILED == -5, "shared surface result values");

int rsf_shared_surface_fixture(void);

int main(void) {
    return rsf_shared_surface_fixture();
}

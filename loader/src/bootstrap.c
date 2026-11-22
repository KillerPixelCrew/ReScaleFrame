// SPDX-License-Identifier: MIT
extern const char* rsf_get_bootstrap_version_impl(void);

// The scaffold performs no initialization from DllMain.
__declspec(dllexport) const char* rsf_get_bootstrap_version(void)
{
    return rsf_get_bootstrap_version_impl();
}

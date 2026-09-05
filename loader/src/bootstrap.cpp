#include <rescaleframe/version.h>

// The scaffold intentionally performs no initialization from DllMain.
extern "C" __declspec(dllexport) const char* rsf_get_bootstrap_version() noexcept
{
    return RSF_VERSION_STRING;
}

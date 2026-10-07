#include <rescaleframe/version.h>

// Version-only bootstrap scaffold. Returns immutable generated storage for the DLL lifetime;
// this export performs no runtime loading or renderer initialization.
extern "C" __declspec(dllexport) const char* rsf_get_bootstrap_version() noexcept
{
    return RSF_VERSION_STRING;
}

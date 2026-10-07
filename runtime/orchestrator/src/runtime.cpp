#include <rescaleframe/runtime.h>
#include <rescaleframe/version.h>

// The generated macro expands to static literal storage, so no allocation crosses the C ABI.
extern "C" const char* rsf_get_runtime_version(void)
{
    return RSF_VERSION_STRING;
}

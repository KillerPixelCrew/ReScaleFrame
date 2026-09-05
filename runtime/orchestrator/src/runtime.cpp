#include <rescaleframe/runtime.h>
#include <rescaleframe/version.h>

extern "C" const char* rsf_get_runtime_version(void)
{
    return RSF_VERSION_STRING;
}

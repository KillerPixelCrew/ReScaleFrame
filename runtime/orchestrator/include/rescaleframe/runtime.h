#pragma once

#ifdef RSF_RUNTIME_BUILD
#define RSF_RUNTIME_API __declspec(dllexport)
#else
#define RSF_RUNTIME_API __declspec(dllimport)
#endif

#ifdef __cplusplus
extern "C" {
#endif

/* Return immutable runtime-owned version text, valid until this DLL unloads.
   Callable without initialization; this entry point performs no graphics or plugin work. */
RSF_RUNTIME_API const char* rsf_get_runtime_version(void);

#ifdef __cplusplus
}
#endif

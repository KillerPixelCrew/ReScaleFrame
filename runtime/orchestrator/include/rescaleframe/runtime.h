#pragma once

#ifdef RSF_RUNTIME_BUILD
#define RSF_RUNTIME_API __declspec(dllexport)
#else
#define RSF_RUNTIME_API __declspec(dllimport)
#endif

#ifdef __cplusplus
extern "C" {
#endif

/* Scaffold metadata only; no runtime initialization occurs. */
RSF_RUNTIME_API const char* rsf_get_runtime_version(void);

#ifdef __cplusplus
}
#endif

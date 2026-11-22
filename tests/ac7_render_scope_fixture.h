/* SPDX-License-Identifier: MIT */
#ifndef RSF_AC7_RENDER_SCOPE_FIXTURE_H
#define RSF_AC7_RENDER_SCOPE_FIXTURE_H
#include <stdbool.h>
#include <rescaleframe/ac7_render_scope.h>

typedef struct rsf_ac7_fixture_lease {
    int resolves;
    int releases;
    bool throw_resolve;
} rsf_ac7_fixture_lease;

#ifdef __cplusplus
extern "C" {
#endif

int rsf_ac7_render_scope_fixture(void);
int rsf_ac7_fixture_enqueue(rsf_ac7_render_scopes*, void* list, rsf_ac7_fixture_lease*);
uint64_t rsf_ac7_fixture_native_frame(const rsf_ac7_render_scope*);
int rsf_ac7_fixture_current(rsf_ac7_render_scopes*);

void rsf_ac7_fixture_throwing_pass(void*, void*, const rsf_ac7_render_scope*, uint32_t);
void rsf_ac7_fixture_throwing_resolve(void*, rsf_ac7_render_scope*);
void rsf_ac7_fixture_throwing_release(void*);

#ifdef __cplusplus
}
#endif
#endif

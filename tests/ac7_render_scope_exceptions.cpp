// SPDX-License-Identifier: MIT
#include "ac7_render_scope_fixture.h"

extern "C" void rsf_ac7_fixture_throwing_pass(void*, void*, const rsf_ac7_render_scope*, uint32_t)
{
    throw 1;
}

extern "C" void rsf_ac7_fixture_throwing_resolve(void* object, rsf_ac7_render_scope* scope)
{
    auto& lease = *static_cast<rsf_ac7_fixture_lease*>(object);
    ++lease.resolves;
    if (lease.throw_resolve) throw 2;
    scope->native_frame += 100;
}

extern "C" void rsf_ac7_fixture_throwing_release(void* object)
{
    ++static_cast<rsf_ac7_fixture_lease*>(object)->releases;
    throw 3;
}

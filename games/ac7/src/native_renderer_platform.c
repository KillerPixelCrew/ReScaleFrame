/* SPDX-License-Identifier: MIT */
/* Native calls of the native renderer (native_renderer.rs), lent at run time through
   rsf_ac7_native_calls_table (ac7_sites.h) so the Rust archive holds no link-time reference to
   them. MinHook, the Windows input facts and the render scope create call are forwarded as they
   are. The pixel shader factory call is the one engine call whose C++ exception the former
   native_renderer.cpp contained; Rust cannot catch a foreign exception, so this leaf filters for
   the MSVC C++ exception code exactly as the former catch (...) under /EHsc did. Structured
   exceptions such as access violations still propagate, as they did before. */
#include "ac7_sites.h"
#include "ac7_scope_owner.h"
#include "../../../integrations/windows/window_input.h"
#include "../../../integrations/windows/xinput_facts.h"
#include <MinHook.h>
#include <windows.h>

static int32_t hook_initialize(void) { return (int32_t)MH_Initialize(); }
static int32_t hook_create(void* target, void* detour, void** original)
{
    return (int32_t)MH_CreateHook(target, detour, original);
}
static int32_t hook_enable(void* target) { return (int32_t)MH_EnableHook(target); }
static int32_t hook_disable(void* target) { return (int32_t)MH_DisableHook(target); }
static int32_t hook_remove(void* target) { return (int32_t)MH_RemoveHook(target); }

static int scopes_create(uint64_t session_id, uint32_t capacity,
    rsf_ac7_render_scope_fn state_callback, rsf_ac7_render_scope_fn pass_callback, void* user,
    void** out)
{
    return rsf_ac7_render_scopes_create_passes(session_id, capacity, state_callback,
        pass_callback, user, (rsf_ac7_render_scopes**)out);
}

typedef void* (*rsf_ac7_shader_factory_fn)(void* rhi, void* result, const void* code);

static bool shader_factory(void* factory, void* rhi, void* result, const void* code, void** created)
{
    const rsf_ac7_shader_factory_fn create = (rsf_ac7_shader_factory_fn)factory;
#if defined(_MSC_VER)
    __try {
#endif
        *created = create(rhi, result, code);
        return true;
#if defined(_MSC_VER)
    } __except (GetExceptionCode() == 0xE06D7363u ? EXCEPTION_EXECUTE_HANDLER
                                                    : EXCEPTION_CONTINUE_SEARCH) {
        return false;
    }
#endif
}

const rsf_ac7_native_calls rsf_ac7_native_calls_table = {
    hook_initialize,
    hook_create,
    hook_enable,
    hook_disable,
    hook_remove,
    rsf_win32_input_kind,
    rsf_xinput_facts_install_loaded,
    rsf_xinput_facts_disable,
    rsf_xinput_facts_remove,
    scopes_create,
    shader_factory,
};

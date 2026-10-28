// SPDX-License-Identifier: GPL-3.0-only
#include "script_input.h"
#include "engine_settings.h"
#include <rescaleframe/render_frontend.h>
#include <rescaleframe/game_api.h>
#include <windows.h>
#include <cstdint>
#include <string>
namespace {
void (*native_init)(uint64_t){};
void (*native_push)(uint64_t){};
uint64_t* (*native_call)(){};
void (*wait)(DWORD){};
uint32_t (*visible)(){};
void (*toggle)(){};
decltype(&rsf_render_settings_get) settings_get{};
bool installed{};
template<class... Args> uint64_t invoke(uint64_t hash, Args... args)
{
    native_init(hash); (native_push(static_cast<uint64_t>(args)), ...);
    const auto* result = native_call(); return result ? *result : 0;
}
void script()
{
    bool previous_key = false, previous_chord = false;
    uint64_t settings_tick{};
    for (;;) {
        const auto now=GetTickCount64();
        if (settings_get && now>=settings_tick) {
            settings_tick=now+250;
            rsf_render_settings prefs{sizeof(prefs),RSF_RENDER_FRONTEND_ABI_VERSION};
            if (settings_get(&prefs)==RSF_OK) rsf_rdr2_engine_quality_tick(prefs.sr_quality);
        }
        DWORD process{}; GetWindowThreadProcessId(GetForegroundWindow(), &process);
        if (process == GetCurrentProcessId()) {
            const bool key = (GetAsyncKeyState(VK_F7) & 0x8000) != 0;
            const bool chord = invoke(0xE2587F8CBBD87B1Dull, 0, 0x43cda5b0) &&
                invoke(0xE2587F8CBBD87B1Dull, 0, 0x7da48d2a);
            if ((key && !previous_key) || (chord && !previous_chord)) toggle();
            previous_key = key; previous_chord = chord;
            // The shared overlay owns drawing and input. ScriptHook only prevents
            // gameplay actions from firing behind its mouse-driven controls.
            if (visible()) invoke(0x5F4B6931816E599Bull, 0);
        } else { previous_key = previous_chord = false; }
        wait(0);
    }
}
}
bool rsf_rdr2_start_script_input(void* instance, void* runtime, const wchar_t* configuration,
    rsf_game_log_fn log, void* user) noexcept
{
    if (installed || !instance || !runtime || !configuration) return false;
    HMODULE library = GetModuleHandleW(L"ScriptHookRDR2.dll");
    if (!library) {
        const std::wstring file = configuration;
        const auto separator = file.find_last_of(L"/\\");
        if (separator != std::wstring::npos)
            library = LoadLibraryW((file.substr(0, separator + 1) + L"ScriptHookRDR2.dll").c_str());
    }
    if (!library) return false;
    auto symbol = [&](const char* name) { return GetProcAddress(library, name); };
    const auto register_script = reinterpret_cast<void (*)(HMODULE, void (*)())>(symbol("?scriptRegister@@YAXPEAUHINSTANCE__@@P6AXXZ@Z"));
    native_init = reinterpret_cast<decltype(native_init)>(symbol("?nativeInit@@YAX_K@Z"));
    native_push = reinterpret_cast<decltype(native_push)>(symbol("?nativePush64@@YAX_K@Z"));
    native_call = reinterpret_cast<decltype(native_call)>(symbol("?nativeCall@@YAPEA_KXZ"));
    wait = reinterpret_cast<decltype(wait)>(symbol("?scriptWait@@YAXK@Z"));
    visible = reinterpret_cast<decltype(visible)>(GetProcAddress(static_cast<HMODULE>(runtime), "rsf_render_overlay_visible"));
    toggle = reinterpret_cast<decltype(toggle)>(GetProcAddress(static_cast<HMODULE>(runtime), "rsf_render_overlay_toggle"));
    settings_get=reinterpret_cast<decltype(settings_get)>(GetProcAddress(static_cast<HMODULE>(runtime),"rsf_render_settings_get"));
    if (!register_script || !native_init || !native_push || !native_call || !wait || !visible || !toggle) return false;
    register_script(static_cast<HMODULE>(instance), script); installed = true;
    if (log) log(user, "RDR2 shared overlay input guard registered; Insert / F7 / both sticks opens the RSF panel.");
    return true;
}

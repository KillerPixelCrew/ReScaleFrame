// SPDX-License-Identifier: GPL-3.0-only
#include <windows.h>
#include <xinput.h>
#include <cstdio>
#include <cwchar>

int main(int argc, char** argv)
{
    if (argc != 2) return 2;
    const auto carrier = LoadLibraryExA(argv[1], nullptr,
        LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS);
    if (!carrier) return 1;
    const char* names[] = {"DllMain", "XInputGetState", "XInputSetState", "XInputGetCapabilities",
        "XInputEnable", "XInputGetDSoundAudioDeviceGuids", "XInputGetBatteryInformation", "XInputGetKeystroke"};
    for (WORD i = 0; i < 8; ++i) {
        const auto named = GetProcAddress(carrier, names[i]);
        if (!named || named != GetProcAddress(carrier, MAKEINTRESOURCEA(i + 1))) return 1;
    }
    for (WORD i = 100; i < 104; ++i) if (!GetProcAddress(carrier, MAKEINTRESOURCEA(i))) return 1;
    wchar_t system[MAX_PATH]{};
    const auto length = GetSystemDirectoryW(system, MAX_PATH);
    if (!length || length + 15 >= MAX_PATH) return 1;
    wcscat_s(system, L"\\xinput1_3.dll");
    const auto original = LoadLibraryExW(system, nullptr, LOAD_LIBRARY_SEARCH_SYSTEM32);
    if (!original) return 1;
    using GetState = DWORD (WINAPI*)(DWORD, XINPUT_STATE*);
    using SetState = DWORD (WINAPI*)(DWORD, XINPUT_VIBRATION*);
    const auto state = reinterpret_cast<GetState>(reinterpret_cast<void*>(GetProcAddress(carrier, MAKEINTRESOURCEA(2))));
    const auto native_state = reinterpret_cast<GetState>(reinterpret_cast<void*>(GetProcAddress(original, MAKEINTRESOURCEA(2))));
    const auto vibration = reinterpret_cast<SetState>(reinterpret_cast<void*>(GetProcAddress(carrier, MAKEINTRESOURCEA(3))));
    const auto native_vibration = reinterpret_cast<SetState>(reinterpret_cast<void*>(GetProcAddress(original, MAKEINTRESOURCEA(3))));
    if (!native_state || !native_vibration) return 1;
    XINPUT_STATE input{}; XINPUT_VIBRATION output{};
    // An invalid user index tests real forwarding without reading or changing any controller.
    if (state(0xffffffffu, &input) != native_state(0xffffffffu, &input) ||
        vibration(0xffffffffu, &output) != native_vibration(0xffffffffu, &output)) return 1;
    FreeLibrary(carrier);
    std::puts("PASS: Wingman XInput names/ordinals and native state/vibration forwarding.");
    return 0;
}

// SPDX-License-Identifier: GPL-3.0-only
// Load the real ASI in stand-in processes. No game installation is touched.
#include <windows.h>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <string>

namespace fs = std::filesystem;

int child()
{
    wchar_t executable[32768]{};
    const auto length = GetModuleFileNameW(nullptr, executable, 32768);
    if (!length || length >= 32768) return 1;
    const auto root = fs::path(executable).parent_path();
    wchar_t expected[32]{};
    GetPrivateProfileStringW(L"Fixture", L"Expected", L"", expected, 32, (root / L"ReScaleFrame.ini").c_str());
    const auto module = LoadLibraryW((root / L"ReScaleFrame.GTA5Enhanced.asi").c_str());
    if (!module) return 1;
    const auto log = root / L"ReScaleFrame" / L"gta5-enhanced.log";
    if (std::wstring(expected) != L"enabled") {
        Sleep(500);
        return fs::exists(log) ? 1 : 0;
    }
    for (unsigned attempt = 0; attempt < 120; ++attempt) {
        std::ifstream stream(log, std::ios::binary);
        const std::string contents{std::istreambuf_iterator<char>(stream), {}};
        if (contents.find("prepare result=-5") != std::string::npos)
            return contents.find("start result=") == std::string::npos ? 0 : 1;
        Sleep(50);
    }
    return 1;
}

bool run_case(const fs::path& base, const fs::path& executable, const fs::path& asi,
              const fs::path& runtime, const fs::path& plugin, const wchar_t* name,
              const wchar_t* target, const wchar_t* expected, bool enabled)
{
    const auto root = base / name;
    const auto mod = root / L"ReScaleFrame";
    fs::create_directories(mod);
    const auto program = root / target;
    fs::copy_file(executable, program);
    fs::copy_file(asi, root / L"ReScaleFrame.GTA5Enhanced.asi");
    fs::copy_file(runtime, mod / L"ReScaleFrame.Runtime.dll");
    fs::copy_file(plugin, mod / L"ReScaleFrame.Game.GTA5Enhanced.dll");
    const auto ini = root / L"ReScaleFrame.ini";
    if (!WritePrivateProfileStringW(L"Fixture", L"Expected", expected, ini.c_str()) ||
        (enabled && !WritePrivateProfileStringW(L"GTA5Enhanced", L"Enable", L"1", ini.c_str()))) return false;
    std::wstring command = L"\"" + program.wstring() + L"\" --child";
    STARTUPINFOW startup{}; startup.cb = sizeof(startup);
    PROCESS_INFORMATION process{};
    if (!CreateProcessW(program.c_str(), command.data(), nullptr, nullptr, FALSE, CREATE_NO_WINDOW,
                        nullptr, root.c_str(), &startup, &process)) return false;
    const auto wait = WaitForSingleObject(process.hProcess, 10000);
    DWORD code = 1;
    const bool passed = wait == WAIT_OBJECT_0 && GetExitCodeProcess(process.hProcess, &code) && code == 0;
    if (wait == WAIT_TIMEOUT) { TerminateProcess(process.hProcess, 1); WaitForSingleObject(process.hProcess, 1000); }
    CloseHandle(process.hThread); CloseHandle(process.hProcess);
    return passed;
}

int wmain(int argc, wchar_t** argv) try
{
    if (argc == 2 && std::wstring(argv[1]) == L"--child") return child();
    if (argc != 5) return 2;
    wchar_t executable[32768]{};
    const auto length = GetModuleFileNameW(nullptr, executable, 32768);
    if (!length || length >= 32768) return 1;
    const auto base = fs::path(argv[4]) / std::to_wstring(GetCurrentProcessId());
    const bool passed = run_case(base, executable, argv[1], argv[2], argv[3], L"disabled",
                                 L"GTA5_Enhanced.exe", L"disabled", false) &&
        run_case(base, executable, argv[1], argv[2], argv[3], L"legacy",
                 L"GTA5.exe", L"legacy", true) &&
        run_case(base, executable, argv[1], argv[2], argv[3], L"enabled",
                 L"GTA5_Enhanced.exe", L"enabled", true);
    if (!passed) { std::cerr << "GTA V Enhanced ASI entry fixture failed.\n"; return 1; }
    std::cout << "ASI defaults off, ignores Legacy, and refuses an unknown Enhanced fingerprint.\n";
    return 0;
} catch (...) { return 1; }

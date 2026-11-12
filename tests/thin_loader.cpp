#include <windows.h>
#include <winioctl.h>
#include <dinput.h>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <map>
#include <atomic>
#include <filesystem>
#include <fstream>
#include <iterator>
#include <string>
#include <thread>
#include <vector>

namespace {
void require(bool ok, const char* message)
{
    if (!ok) {
        std::fprintf(stderr, "FAIL: %s (Win32=%lu)\n", message, GetLastError());
        std::exit(1);
    }
}
std::map<std::string, DWORD> exports(HMODULE module)
{
    const auto* base = reinterpret_cast<const BYTE*>(module);
    const auto* dos = reinterpret_cast<const IMAGE_DOS_HEADER*>(base);
    const auto* nt = reinterpret_cast<const IMAGE_NT_HEADERS*>(base + dos->e_lfanew);
    const auto* table = reinterpret_cast<const IMAGE_EXPORT_DIRECTORY*>(base +
        nt->OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_EXPORT].VirtualAddress);
    require(table->NumberOfNames == table->NumberOfFunctions, "no unnamed or extra exports");
    const auto* names = reinterpret_cast<const DWORD*>(base + table->AddressOfNames);
    const auto* ordinals = reinterpret_cast<const WORD*>(base + table->AddressOfNameOrdinals);
    std::map<std::string, DWORD> result;
    for (DWORD i = 0; i < table->NumberOfNames; ++i)
        result.emplace(reinterpret_cast<const char*>(base + names[i]), table->Base + ordinals[i]);
    return result;
}
void pe_test(const wchar_t* path)
{
    HMODULE carrier = LoadLibraryExW(path, nullptr, DONT_RESOLVE_DLL_REFERENCES);
    require(carrier != nullptr, "thin carrier exists and maps as PE");
    wchar_t system[MAX_PATH];
    require(GetSystemDirectoryW(system, MAX_PATH) != 0, "get system directory");
    std::wstring system_path = std::wstring(system) + L"\\dinput8.dll";
    HMODULE genuine = LoadLibraryExW(system_path.c_str(), nullptr, DONT_RESOLVE_DLL_REFERENCES);
    require(genuine != nullptr, "map genuine System32 DirectInput");
    require(exports(carrier) == exports(genuine), "export names and ordinals match real System32 DLL");
    const auto* base = reinterpret_cast<const BYTE*>(carrier);
    const auto* dos = reinterpret_cast<const IMAGE_DOS_HEADER*>(base);
    const auto* nt = reinterpret_cast<const IMAGE_NT_HEADERS*>(base + dos->e_lfanew);
    require(nt->FileHeader.Machine == IMAGE_FILE_MACHINE_AMD64, "native x64 image");
    const DWORD import_rva = nt->OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_IMPORT].VirtualAddress;
    require(import_rva != 0, "imports exist");
    const auto* imports = reinterpret_cast<const IMAGE_IMPORT_DESCRIPTOR*>(base + import_rva);
    for (; imports->Name; ++imports) {
        const char* name = reinterpret_cast<const char*>(base + imports->Name);
        const bool allowed = _stricmp(name, "kernel32.dll") == 0 ||
            _stricmp(name, "vcruntime140.dll") == 0 || _stricmp(name, "vcruntime140d.dll") == 0 ||
            _stricmp(name, "vcruntime140_1.dll") == 0 || _stricmp(name, "vcruntime140_1d.dll") == 0 ||
            _stricmp(name, "msvcp140.dll") == 0 || _stricmp(name, "msvcp140d.dll") == 0 ||
            _stricmp(name, "ucrtbased.dll") == 0 ||
            _strnicmp(name, "api-ms-win-crt-", 15) == 0;
        if (!allowed) std::fprintf(stderr, "unexpected import: %s\n", name);
        require(allowed, "only OS/CRT imports; no runtime, graphics, or adapter dependency");
    }
    require(nt->OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_DELAY_IMPORT].VirtualAddress == 0,
        "no hidden delay imports");
    std::string image(reinterpret_cast<const char*>(base), nt->OptionalHeader.SizeOfImage);
    for (char& c : image) if (c >= 'A' && c <= 'Z') c += 'a' - 'A';
    require(image.find("anomaly") == std::string::npos &&
        image.find("orchestrator") == std::string::npos && image.find("ac7") == std::string::npos,
        "carrier embeds no game identity or orchestrator code/debug path");
    FreeLibrary(genuine);
    FreeLibrary(carrier);
}
template<typename T> T proc(HMODULE module, const char* name)
{
    FARPROC address = GetProcAddress(module, name);
    require(address != nullptr, "required export resolves");
    T result;
    static_assert(sizeof(result) == sizeof(address));
    std::memcpy(&result, &address, sizeof(result));
    return result;
}
void forward_test(const wchar_t* path)
{
    HMODULE carrier = LoadLibraryW(path);
    wchar_t system[MAX_PATH];
    require(GetSystemDirectoryW(system, MAX_PATH) != 0, "get system directory");
    const auto system_path = std::wstring(system) + L"\\dinput8.dll";
    HMODULE genuine = LoadLibraryExW(system_path.c_str(), nullptr, LOAD_LIBRARY_SEARCH_SYSTEM32);
    require(carrier && genuine && carrier != genuine, "load distinct proxy and genuine system DLL");
    for (const auto& item : exports(genuine))
        require(GetProcAddress(carrier, item.first.c_str()) ==
            GetProcAddress(carrier, MAKEINTRESOURCEA(item.second)), "ordinal aliases same export");
    using Create = HRESULT (WINAPI*)(HINSTANCE, DWORD, REFIID, LPVOID*, LPUNKNOWN);
    const auto create = proc<Create>(carrier, "DirectInput8Create");
    const auto real_create = proc<Create>(genuine, "DirectInput8Create");
    void* output = nullptr;
    void* real_output = nullptr;
    require(create(GetModuleHandleW(nullptr), 0, IID_IDirectInput8W, &output, nullptr) ==
        real_create(GetModuleHandleW(nullptr), 0, IID_IDirectInput8W, &real_output, nullptr),
        "invalid DirectInput version HRESULT forwarded exactly");
    require(create(GetModuleHandleW(nullptr), DIRECTINPUT_VERSION, IID_IDirectInput8W,
        &output, nullptr) == S_OK && output, "genuine DirectInput8 COM object created through carrier");
    static_cast<IDirectInput8W*>(output)->Release();
    using NoArgs = HRESULT (WINAPI*)();
    require(proc<NoArgs>(carrier, "DllCanUnloadNow")() == proc<NoArgs>(genuine, "DllCanUnloadNow")(),
        "DllCanUnloadNow system HRESULT");
    using ClassObject = HRESULT (WINAPI*)(REFCLSID, REFIID, LPVOID*);
    require(proc<ClassObject>(carrier, "DllGetClassObject")(GUID_NULL, GUID_NULL, &output) ==
        proc<ClassObject>(genuine, "DllGetClassObject")(GUID_NULL, GUID_NULL, &real_output),
        "DllGetClassObject system HRESULT");
    using Format = const void* (WINAPI*)();
    require(proc<Format>(carrier, "GetdfDIJoystick")() == proc<Format>(genuine, "GetdfDIJoystick")(),
        "joystick data format pointer owned by genuine DLL");
    FreeLibrary(carrier);
    FreeLibrary(genuine);
}
std::string read_text(const std::filesystem::path& path)
{
    std::ifstream file(path, std::ios::binary);
    return {std::istreambuf_iterator<char>(file), std::istreambuf_iterator<char>()};
}
std::wstring final_path(const std::filesystem::path& path)
{
    HANDLE file = CreateFileW(path.c_str(), GENERIC_READ, FILE_SHARE_READ, nullptr, OPEN_EXISTING,
        FILE_ATTRIBUTE_NORMAL, nullptr);
    require(file != INVALID_HANDLE_VALUE, "open fixture configuration");
    wchar_t result[32768];
    const DWORD length = GetFinalPathNameByHandleW(file, result, _countof(result), FILE_NAME_NORMALIZED);
    CloseHandle(file);
    require(length && length < _countof(result), "canonical fixture config path");
    return result;
}
void junction(const std::filesystem::path& link, const std::filesystem::path& target)
{
    require(std::filesystem::create_directory(link), "create junction directory");
    auto native_target = target;
    native_target.make_preferred();
    const auto substitute = std::wstring(L"\\??\\") + native_target.wstring();
    struct Header {
        DWORD tag;
        WORD length, reserved;
        WORD substitute_offset, substitute_length, print_offset, print_length;
    };
    const auto bytes = static_cast<WORD>(substitute.size() * sizeof(wchar_t));
    Header header{IO_REPARSE_TAG_MOUNT_POINT, static_cast<WORD>(8 + bytes + 4), 0, 0, bytes,
        static_cast<WORD>(bytes + 2), 0};
    std::vector<BYTE> buffer(sizeof(header) + bytes + 4, 0);
    std::memcpy(buffer.data(), &header, sizeof(header));
    std::memcpy(buffer.data() + sizeof(header), substitute.data(), bytes);
    HANDLE directory = CreateFileW(link.c_str(), GENERIC_WRITE, 0, nullptr, OPEN_EXISTING,
        FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS, nullptr);
    require(directory != INVALID_HANDLE_VALUE, "open junction");
    DWORD returned = 0;
    const BOOL ok = DeviceIoControl(directory, FSCTL_SET_REPARSE_POINT, buffer.data(),
        static_cast<DWORD>(buffer.size()), nullptr, 0, &returned, nullptr);
    CloseHandle(directory);
    require(ok != 0, "create native unprivileged junction fixture");
}
class LoaderUiWatch {
    std::atomic<bool> stop{false};
    std::thread watcher;
    static BOOL CALLBACK window(HWND window, LPARAM)
    {
        DWORD pid = 0;
        GetWindowThreadProcessId(window, &pid);
        if (pid == GetCurrentProcessId() && IsWindowVisible(window)) {
            std::fprintf(stderr, "FAIL: loader displayed a visible window in its test subprocess\n");
            // Only this test process is terminated. Never touch another application's windows.
            ExitProcess(2);
        }
        return TRUE;
    }
public:
    LoaderUiWatch() : watcher([this]() {
        const ULONGLONG deadline = GetTickCount64() + 4000;
        while (!stop.load()) {
            EnumWindows(window, 0);
            if (GetTickCount64() >= deadline) {
                std::fprintf(stderr, "FAIL: native loader exceeded four-second no-UI deadline\n");
                ExitProcess(2);
            }
            Sleep(2);
        }
    }) {}
    ~LoaderUiWatch() { stop.store(true); watcher.join(); }
    LoaderUiWatch(const LoaderUiWatch&) = delete;
    LoaderUiWatch& operator=(const LoaderUiWatch&) = delete;
};
void startup_test(const wchar_t* mode, const wchar_t* carrier_path,
    const wchar_t* runtime_path, const wchar_t* adapter_path)
{
    namespace fs = std::filesystem;
    const fs::path source(carrier_path);
    // Fixtures retain loaded DLLs until exit. A reused PID must not select an older root.
    const auto nonce=std::to_string(GetCurrentProcessId())+"-"+std::to_string(GetTickCount64());
    const std::wstring wide_nonce(nonce.begin(),nonce.end());
    const auto root = source.parent_path() / (std::wstring(L"fixture-") + mode + L"-" +
        wide_nonce + L"-\u00e9");
    require(fs::create_directories(root / L"ReScaleFrame"), "create isolated Unicode fixture root");
    fs::copy_file(source, root / L"dinput8.dll");
    fs::copy_file(runtime_path, root / L"ReScaleFrame" / L"RuntimeOne.dll");
    // Decoy default runtime proves an explicit selection never also loads the default.
    fs::copy_file(runtime_path, root / L"ReScaleFrame" / L"ReScaleFrame.Runtime.dll");
    fs::copy_file(adapter_path, root / L"ReScaleFrame" / L"AdapterOne.dll");
    const auto configuration = root / L"ReScaleFrame.ini";
    const bool malformed = std::wcscmp(mode, L"malformed") == 0;
    const bool duplicate = std::wcscmp(mode, L"duplicate") == 0;
    const bool escape = std::wcscmp(mode, L"escape") == 0;
    const bool reparse = std::wcscmp(mode, L"junction") == 0;
    if (reparse) {
        const auto outside = root.parent_path() / (L"outside-" + wide_nonce);
        require(fs::create_directory(outside), "create outside junction target");
        fs::copy_file(runtime_path, outside / L"RuntimeOne.dll");
        junction(root / L"Linked", outside);
        require(fs::exists(root / L"Linked" / L"RuntimeOne.dll"), "junction really resolves to outside DLL");
    }
    const auto outside_name = L"OutsideRuntime-" + wide_nonce + L".dll";
    if (escape) fs::copy_file(runtime_path, root.parent_path() / outside_name);
    {
        std::ofstream ini(configuration, std::ios::binary);
        ini << "[Loader]\nAutoStart=" << (malformed ? "1trailing" : "1") << "\nRuntimeModule=";
        if (escape) ini << "../OutsideRuntime-" << nonce << ".dll\n";
        else if (reparse) ini << "Linked/RuntimeOne.dll\n";
        else ini << "ReScaleFrame/RuntimeOne.dll\n";
        ini << "EntryModule=ReScaleFrame/AdapterOne.dll\nEntryPoint=ThinFixtureStart\n";
        if (duplicate) ini << "aUtOsTaRt=0\n";
    }
    auto configured = [](const std::string& runtime = "ReScaleFrame/RuntimeOne.dll",
        const std::string& adapter = "ReScaleFrame/AdapterOne.dll",
        const std::string& entry = "ThinFixtureStart") {
        return "[Loader]\nAutoStart=1\nRuntimeModule=" + runtime + "\nEntryModule=" + adapter +
            "\nEntryPoint=" + entry + "\n";
    };
    const std::string valid_ini = configured();
    const std::map<std::wstring, std::string> invalid_configs{
        {L"empty", ""}, {L"missing_section", "[Other]\nAutoStart=1\n"},
        {L"missing_auto", "[Loader]\nEntryModule=ReScaleFrame/AdapterOne.dll\nEntryPoint=ThinFixtureStart\n"},
        {L"missing_entry", "[Loader]\nAutoStart=1\nRuntimeModule=ReScaleFrame/RuntimeOne.dll\n"},
        {L"empty_runtime", configured("")}, {L"empty_entry", configured("ReScaleFrame/RuntimeOne.dll", "")},
        {L"invalid_symbol", configured("ReScaleFrame/RuntimeOne.dll", "ReScaleFrame/AdapterOne.dll", "Bad Export")},
        {L"unknown_key", valid_ini + "RunAnotherRuntime=yes\n"},
        {L"duplicate_section", valid_ini + "[lOaDeR]\nAutoStart=1\n"},
        {L"syntax", valid_ini + "not a key value pair\n"},
        {L"nul", valid_ini + std::string("\0ignored", 8)},
        {L"bad_utf8", valid_ini + std::string("\xff", 1)},
        {L"oversized", valid_ini + std::string(65537, ';')},
        {L"absolute", configured("C:/Windows/System32/dinput8.dll")},
        {L"rooted", configured("/Windows/System32/dinput8.dll")},
        {L"drive_relative", configured("C:RuntimeOne.dll")},
        {L"unc", configured("\\\\localhost\\share\\RuntimeOne.dll")},
        {L"ads", configured("ReScaleFrame/RuntimeOne.dll:stream")},
        {L"trailing_dot", configured("ReScaleFrame/RuntimeOne.dll.")},
        {L"empty_component", configured("ReScaleFrame//RuntimeOne.dll")},
        {L"parent_component", configured("ReScaleFrame/../ReScaleFrame/RuntimeOne.dll")},
        {L"adapter_escape", configured("ReScaleFrame/RuntimeOne.dll", "../OutsideAdapter.dll")},
        {L"same_module", configured("ReScaleFrame/RuntimeOne.dll", "ReScaleFrame/RuntimeOne.dll")},
        {L"self_module", configured("dinput8.dll")}
    };
    std::string expected_events = (malformed || duplicate || escape || reparse) ? "" : "RAE";
    if (const auto found = invalid_configs.find(mode); found != invalid_configs.end()) {
        std::ofstream(configuration, std::ios::binary) << found->second;
        expected_events.clear();
    }
    if (std::wcscmp(mode, L"disabled") == 0) {
        std::ofstream(configuration, std::ios::binary) << "[Loader]\nAutoStart=false\n";
        expected_events.clear();
    }
    const bool default_runtime = std::wcscmp(mode, L"default") == 0;
    if (default_runtime)
        std::ofstream(configuration, std::ios::binary) << "[Loader]\nAutoStart=true\n"
            "EntryModule=ReScaleFrame/AdapterOne.dll\nEntryPoint=ThinFixtureStart\n";
    if (std::wcscmp(mode, L"utf8_bom") == 0)
        std::ofstream(configuration, std::ios::binary) << "\xef\xbb\xbf" << valid_ini;
    if (std::wcscmp(mode, L"utf16") == 0) {
        const std::wstring wide(valid_ini.begin(), valid_ini.end());
        std::ofstream ini(configuration, std::ios::binary);
        ini.write("\xff\xfe", 2);
        ini.write(reinterpret_cast<const char*>(wide.data()), static_cast<std::streamsize>(wide.size() * 2));
    }
    if (std::wcscmp(mode, L"shared_config") == 0)
        std::ofstream(configuration, std::ios::binary) << "; comment\n[Other]\nUnrelated=ok\n" << valid_ini;
    if (std::wcscmp(mode, L"missing_export") == 0) {
        std::ofstream(configuration, std::ios::binary) << configured("ReScaleFrame/RuntimeOne.dll",
            "ReScaleFrame/AdapterOne.dll", "MissingExport");
        expected_events = "RA";
    }
    if (std::wcscmp(mode, L"hardlink_alias") == 0) {
        fs::remove(root / L"ReScaleFrame" / L"AdapterOne.dll");
        fs::create_hard_link(root / L"ReScaleFrame" / L"RuntimeOne.dll", root / L"ReScaleFrame" / L"AdapterOne.dll");
        expected_events.clear();
    }
    const bool scoped_modes = std::wcscmp(mode, L"scoped_modes") == 0;
    const DWORD initial_thread_mode = GetThreadErrorMode();
    const DWORD initial_process_mode = GetErrorMode();
    SetEnvironmentVariableW(L"RSF_THIN_FIXTURE_CHECK_ERROR_MODE", scoped_modes ? L"1" : nullptr);
    SetEnvironmentVariableW(L"RSF_THIN_FIXTURE_THREAD_MODE", std::to_wstring(initial_thread_mode).c_str());
    SetEnvironmentVariableW(L"RSF_THIN_FIXTURE_PROCESS_MODE", std::to_wstring(initial_process_mode).c_str());
    const bool refuse = std::wcscmp(mode, L"refuse") == 0;
    SetEnvironmentVariableW(L"RSF_THIN_FIXTURE_REFUSE", refuse ? L"1" : nullptr);
    const auto events = root / L"events.txt";
    require(SetEnvironmentVariableW(L"RSF_THIN_FIXTURE_EVENTS", events.c_str()) != 0, "fixture events env");
    require(SetEnvironmentVariableW(L"RSF_THIN_FIXTURE_CONFIG", final_path(configuration).c_str()) != 0,
        "fixture config env");
    if (std::wcscmp(mode, L"missing_config") == 0) {
        fs::remove(configuration);
        expected_events.clear();
    }
    if (std::wcscmp(mode, L"missing_runtime") == 0 || std::wcscmp(mode, L"missing_adapter") == 0) {
        fs::remove(root / L"ReScaleFrame" /
            (std::wcscmp(mode, L"missing_runtime") == 0 ? L"RuntimeOne.dll" : L"AdapterOne.dll"));
        expected_events.clear();
    }
    if (std::wcscmp(mode, L"bad_runtime_image") == 0 || std::wcscmp(mode, L"bad_adapter_image") == 0) {
        const bool bad_runtime = std::wcscmp(mode, L"bad_runtime_image") == 0;
        std::ofstream(root / L"ReScaleFrame" / (bad_runtime ? L"RuntimeOne.dll" : L"AdapterOne.dll"),
            std::ios::binary) << "not a PE image";
        expected_events = bad_runtime ? "" : "R";
    }
    void* output = nullptr;
    wchar_t pointer[64];
    swprintf_s(pointer, L"%llx", static_cast<unsigned long long>(reinterpret_cast<uintptr_t>(&output)));
    SetEnvironmentVariableW(L"RSF_THIN_FIXTURE_OUTPUT", pointer);
    SetEnvironmentVariableW(L"RSF_THIN_FIXTURE_THREAD", std::to_wstring(GetCurrentThreadId()).c_str());
    HMODULE carrier = LoadLibraryW((root / L"dinput8.dll").c_str());
    require(carrier != nullptr, "load copied carrier");
    Sleep(120);
    require(read_text(events).empty(), "DllMain never starts configured modules or a background worker");
    proc<HRESULT (WINAPI*)()>(carrier, "DllCanUnloadNow")();
    require(read_text(events).empty(), "non-create export never starts modules");
    using Create = HRESULT (WINAPI*)(HINSTANCE, DWORD, REFIID, LPVOID*, LPUNKNOWN);
    const auto create = proc<Create>(carrier, "DirectInput8Create");
    LoaderUiWatch no_loader_ui;
    if (std::wcscmp(mode, L"concurrent") == 0) {
        SetEnvironmentVariableW(L"RSF_THIN_FIXTURE_THREAD", nullptr);
        HANDLE gate = CreateEventW(nullptr, TRUE, FALSE, nullptr);
        require(gate != nullptr, "create concurrent startup gate");
        std::vector<std::thread> workers;
        for (int i = 0; i < 16; ++i) workers.emplace_back([&]() {
            require(WaitForSingleObject(gate, 5000) == WAIT_OBJECT_0, "wait simultaneous create");
            void* local_output = nullptr;
            require(create(GetModuleHandleW(nullptr), DIRECTINPUT_VERSION, IID_IDirectInput8W,
                &local_output, nullptr) == S_OK && local_output, "concurrent real DirectInput object");
            require(read_text(events) == "RAE", "every concurrent return waits for adapter completion");
            static_cast<IDirectInput8W*>(local_output)->Release();
        });
        SetEvent(gate);
        for (auto& worker : workers) worker.join();
        CloseHandle(gate);
    }
    require(create(GetModuleHandleW(nullptr), DIRECTINPUT_VERSION, IID_IDirectInput8W,
        &output, nullptr) == S_OK && output, "system DirectInput still succeeds");
    require(read_text(events) == expected_events, "only valid configuration starts runtime then entry before real create");
    require(GetThreadErrorMode() == initial_thread_mode && GetErrorMode() == initial_process_mode,
        "all startup success/refusal paths restore caller thread and preserve process error modes");
    static_cast<IDirectInput8W*>(output)->Release();
    // A fixed config after refusal must not trigger a second startup attempt.
    if (expected_events.empty()) std::ofstream(configuration, std::ios::binary) << valid_ini;
    output = nullptr;
    require(create(GetModuleHandleW(nullptr), DIRECTINPUT_VERSION, IID_IDirectInput8W,
        &output, nullptr) == S_OK && output, "second forwarded create succeeds");
    static_cast<IDirectInput8W*>(output)->Release();
    require(read_text(events) == expected_events, "startup success or refusal is final exactly once");
    const bool runtime_loaded = expected_events.find('R') != std::string::npos;
    const bool adapter_loaded = expected_events.find('A') != std::string::npos;
    const wchar_t* runtime_name = default_runtime ? L"ReScaleFrame.Runtime.dll" : L"RuntimeOne.dll";
    auto check_retention = [&]() {
        require((GetModuleHandleW(runtime_name) != nullptr) == runtime_loaded &&
            (GetModuleHandleW(L"AdapterOne.dll") != nullptr) == adapter_loaded,
            "loaded modules persist even after refusal and carrier release");
        require(!GetModuleHandleW(default_runtime ? L"RuntimeOne.dll" : L"ReScaleFrame.Runtime.dll"),
            "exactly one selected runtime; no default/explicit fallback or double load");
    };
    check_retention();
    const auto diagnostic = read_text(root / L"ReScaleFrame.Loader.log");
    require(!diagnostic.empty(), "startup diagnostic persisted");
    if (refuse) require(diagnostic.find("entry refused status=7") != std::string::npos,
        "adapter refusal status logged while system forwarding continues");
    if (std::wcscmp(mode, L"missing_export") == 0)
        require(diagnostic.find("entry export missing") != std::string::npos, "missing export diagnosed");
    if (std::wcscmp(mode, L"bad_runtime_image") == 0 || std::wcscmp(mode, L"bad_adapter_image") == 0)
        require(diagnostic.find("load failed win32=193") != std::string::npos,
            "actual ERROR_BAD_EXE_FORMAT preserved across thread mode restoration and diagnosed");
    if (expected_events.empty()) require(diagnostic.find("entry ready") == std::string::npos,
        "refusal never logged as successful startup");
    FreeLibrary(carrier);
    check_retention();
}
}
int wmain(int argc, wchar_t** argv)
{
    // Loader UI suppression is tested at the actual LoadLibrary boundary, not process-wide here.
    require(argc == 3 || argc == 5, "usage: thin_loader_test <mode> <carrier> [runtime adapter]");
    if (argc == 5) startup_test(argv[1], argv[2], argv[3], argv[4]);
    else if (std::wcscmp(argv[1], L"pe") == 0) pe_test(argv[2]);
    else if (std::wcscmp(argv[1], L"forward") == 0) forward_test(argv[2]);
    else require(false, "known mode");
    std::puts("PASS thin loader contract");
}

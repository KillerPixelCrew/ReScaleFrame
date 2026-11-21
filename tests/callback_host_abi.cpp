// SPDX-License-Identifier: MIT
// CPU-only refusal contract for rsf_callback_host_start/stop, linked once against the Runtime DLL
// and once against the embedded core. No plugin loads and no device is created: every start below
// is refused before or at plugin preparation.
#include <rescaleframe/callback_render_host.h>
#include <rescaleframe/sr_session.h>
#include <windows.h>
#include <cstdio>
#include <fstream>
#include <iterator>
#include <string>

namespace {
int policy_calls = 0;
uint32_t policy_spatial = 99, policy_quality = 99;
int policy_result = 1;
uint32_t reentrant_stop = 99;
int policy(uint32_t spatial, uint32_t quality)
{
    ++policy_calls; policy_spatial = spatial; policy_quality = quality;
    reentrant_stop = rsf_callback_host_stop();
    return policy_result;
}
bool ok = true;
void check(bool condition, const char* what)
{
    if (!condition) { std::fprintf(stderr, "callback host ABI: %s\n", what); ok = false; }
}
std::wstring temporary(const wchar_t* name)
{
    wchar_t directory[MAX_PATH]{};
    GetTempPathW(MAX_PATH, directory);
    std::wstring path = directory;
    path += L"rsf-callback-host-abi-" + std::to_wstring(GetCurrentProcessId()) + L"-" + name;
    DeleteFileW(path.c_str());
    return path;
}
std::string contents(const std::wstring& path)
{
    std::ifstream file(path, std::ios::binary);
    return {std::istreambuf_iterator<char>(file), std::istreambuf_iterator<char>()};
}
}

int main()
{
    const auto log = temporary(L"host.log"), preferences = temporary(L"preferences.ini");
    const auto missing_plugin = temporary(L"missing-plugin.dll");
    const rsf_game_probe probe{sizeof(probe), 0x8664, 0, "Game.exe", "00"};
    rsf_callback_host_setup valid{};
    valid.struct_size = sizeof(valid); valid.abi_version = RSF_CALLBACK_HOST_ABI_VERSION;
    valid.plugin_path = missing_plugin.c_str(); valid.log_path = log.c_str();
    valid.preferences_path = preferences.c_str(); valid.probe = &probe;
    valid.backend = RSF_SR_AUTO; valid.quality = 2; valid.dlss_preset = 0;
    valid.sr_backend_choices = (1u << RSF_SR_DLSS) | (1u << RSF_SR_FSR3) | (1u << RSF_SR_SPATIAL);
    valid.engine_version_utf8 = "fixture"; valid.project_id_utf8 = "fixture";
    valid.set_engine_spatial = policy;

    check(rsf_callback_host_stop() == 0, "stop without a host");
    check(rsf_callback_host_start(nullptr) == 1, "null setup");
    auto setup = valid; setup.struct_size -= 8;
    check(rsf_callback_host_start(&setup) == 1, "short setup");
    setup = valid; setup.abi_version = 2;
    check(rsf_callback_host_start(&setup) == 1, "ABI mismatch");
    setup = valid; setup.probe = nullptr;
    check(rsf_callback_host_start(&setup) == 1, "missing probe");
    setup = valid; setup.set_engine_spatial = nullptr;
    check(rsf_callback_host_start(&setup) == 1, "missing adapter policy");
    setup = valid; setup.sr_backend_choices = 0;
    check(rsf_callback_host_start(&setup) == 1, "no SR choices");
    setup = valid; setup.dlss_preset = 0x100;
    check(rsf_callback_host_start(&setup) == 2, "invalid DLSS preset");
    setup = valid; setup.backend = RSF_SR_AUTO + 1;
    check(rsf_callback_host_start(&setup) == 2, "invalid backend");
    setup = valid; setup.backend = RSF_SR_DLSS; setup.quality = 5;
    check(rsf_callback_host_start(&setup) == 2, "DLSS ultra quality");
    check(policy_calls == 0, "policy consulted before validation");
    const std::wstring unwritable = L"Z:\\rsf-missing-directory\\host.log";
    setup = valid; setup.log_path = unwritable.c_str();
    check(rsf_callback_host_start(&setup) == 4, "log unavailable");
    check(policy_calls == 0, "policy consulted before the log opened");

    policy_result = 0;
    check(rsf_callback_host_start(&valid) == 8, "adapter policy refusal");
    check(policy_calls == 1 && policy_spatial == 0 && policy_quality == 2, "Auto asks for native rendering");
    check(reentrant_stop == 1, "adapter policy callback cannot retire the host under construction");
    policy_result = 1;
    setup = valid; setup.backend = RSF_SR_SPATIAL; setup.quality = 5;
    check(rsf_callback_host_start(&setup) == 10, "missing plugin refused");
    check(policy_calls == 2 && policy_spatial == 1 && policy_quality == 5, "spatial policy query");
    const std::wstring relative = L"Adapter.dll";
    setup = valid; setup.plugin_path = relative.c_str();
    check(rsf_callback_host_start(&setup) == 10, "relative plugin refused");
    check(rsf_callback_host_stop() == 0, "refused starts publish no host");
    const auto text = contents(log);
    check(text.find("Native renderer plugin preparation/activation refused.\r\n") != std::string::npos,
        "refusal logged");
    DeleteFileW(log.c_str()); DeleteFileW(preferences.c_str());
    if (ok) std::puts("Callback host refusals and stop verified without a plugin or device.");
    return ok ? 0 : 1;
}

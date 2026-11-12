/* SPDX-License-Identifier: GPL-3.0-only */
#include <rescaleframe/d3d11_present_api.h>
#include <windows.h>
#include <cstdio>
#include <cstring>
#include <cstdint>
#include <cstddef>
#include <initializer_list>
#include <type_traits>
#include <thread>
#include <atomic>
#include <array>

#ifdef RSF_RUNTIME_BUILD
#error This contract must import the real Runtime DLL, not compile as its owner.
#endif

namespace {
int failures = 0;
void check(bool condition, const char* message) {
    if (!condition) {
        ++failures;
        std::fprintf(stderr, "FAIL: %s\n", message);
    }
}

void refuses_without_writes(uint32_t size, uint32_t version, rsf_result expected) {
    rsf_d3d11_present_api api;
    std::memset(&api, 0xa5, sizeof(api));
    api.struct_size = size;
    api.abi_version = version;
    unsigned char before[sizeof(api)];
    std::memcpy(before, &api, sizeof(api));
    check(rsf_get_d3d11_present_api(&api) == expected, "deterministic ABI refusal result");
    check(std::memcmp(before, &api, sizeof(api)) == 0, "refusal preserves every caller byte");
}

void refusal_contract() {
    check(rsf_get_d3d11_present_api(nullptr) == RSF_ERROR_INVALID_ARGUMENT,
          "null query returns INVALID_ARGUMENT");
    const uint32_t short_sizes[] = {0u, 1u, 3u, 4u, 7u, 8u,
                                    uint32_t(sizeof(rsf_d3d11_present_api) - 1)};
    for (const auto size : short_sizes) {
        refuses_without_writes(size, RSF_D3D11_PRESENT_API_VERSION, RSF_ERROR_INVALID_ARGUMENT);
        refuses_without_writes(size, UINT32_MAX, RSF_ERROR_INVALID_ARGUMENT);
    }
    for (const auto version : {0u, RSF_D3D11_PRESENT_API_VERSION + 1u, UINT32_MAX}) {
        refuses_without_writes(sizeof(rsf_d3d11_present_api), version, RSF_ERROR_ABI_MISMATCH);
        refuses_without_writes(UINT32_MAX, version, RSF_ERROR_ABI_MISMATCH);
    }
}

/* Only an aligned size/version header is accessible. The function slots are in
   a protected page; the header itself is read-only while refusal is exercised. */
void refusal_prefix_guard_page() {
    SYSTEM_INFO info{};
    GetSystemInfo(&info);
    const auto page = size_t(info.dwPageSize);
    auto* memory = static_cast<unsigned char*>(VirtualAlloc(nullptr, page * 2,
        MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE));
    check(memory != nullptr, "allocate guard-page refusal fixture");
    if (!memory) return;
    auto* prefix = memory + page - 2 * sizeof(uint32_t);
    DWORD old = 0;
    const bool guarded = VirtualProtect(memory + page, page, PAGE_NOACCESS, &old) != 0;
    check(guarded, "protect inaccessible function-slot page");
    if (guarded) {
        for (const auto size : {uint32_t(sizeof(rsf_d3d11_present_api) - 1),
                                uint32_t(sizeof(rsf_d3d11_present_api))}) {
            const uint32_t header[] = {size, UINT32_MAX};
            std::memcpy(prefix, header, sizeof(header));
            const bool readonly = VirtualProtect(memory, page, PAGE_READONLY, &old) != 0;
            check(readonly, "protect refusal header against writes");
            if (readonly) {
                const auto expected = size < sizeof(rsf_d3d11_present_api) ?
                    RSF_ERROR_INVALID_ARGUMENT : RSF_ERROR_ABI_MISMATCH;
                check(rsf_get_d3d11_present_api(reinterpret_cast<rsf_d3d11_present_api*>(prefix)) ==
                          expected, "refusal reads only available header and writes nothing");
                check(std::memcmp(prefix, header, sizeof(header)) == 0,
                      "refusal preserves read-only header bytes");
                check(VirtualProtect(memory, page, PAGE_READWRITE, &old) != 0,
                      "restore fixture header writes");
            }
        }
    }
    VirtualFree(memory, 0, MEM_RELEASE);
}
void populated_table_contract() {
    const auto runtime = GetModuleHandleW(L"ReScaleFrame.Runtime.dll");
    check(runtime != nullptr, "real Runtime DLL is imported");
    check(GetProcAddress(runtime, RSF_D3D11_PRESENT_API_ENTRY_POINT) != nullptr,
          "query has an undecorated C export");
    rsf_d3d11_present_api api{};
    api.struct_size = sizeof(api);
    api.abi_version = RSF_D3D11_PRESENT_API_VERSION;
    const auto result = rsf_get_d3d11_present_api(&api);
    check(result == RSF_OK, "compatible query succeeds");
    if (result != RSF_OK) return;
    check(api.struct_size == sizeof(api), "query reports implemented table size");
    check(api.abi_version == RSF_D3D11_PRESENT_API_VERSION, "query reports implemented version");

    // Compare every slot's type to the existing bridge, without linking or calling it.
    // Module identity verifies pointers belong to this DLL, not a client-side owner.
#define RSF_CHECK_SLOT(name) \
    static_assert(std::is_same_v<decltype(api.name), decltype(&rsf_d3d11_present_##name)>); \
    check(api.name != nullptr, #name " slot is populated"); \
    if (api.name) { \
        HMODULE owner = nullptr; \
        const bool found = GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | \
            GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT, \
            reinterpret_cast<LPCWSTR>(api.name), &owner) != 0; \
        check(found && owner == runtime, #name " belongs to imported Runtime DLL"); \
    }
    RSF_CHECK_SLOT(install)
    RSF_CHECK_SLOT(prepare)
    RSF_CHECK_SLOT(create_for_hwnd)
    RSF_CHECK_SLOT(host)
    RSF_CHECK_SLOT(graphics)
    RSF_CHECK_SLOT(queue)
    RSF_CHECK_SLOT(command_list)
    RSF_CHECK_SLOT(retire)
    RSF_CHECK_SLOT(is_owner)
    RSF_CHECK_SLOT(has_owner)
    RSF_CHECK_SLOT(provider)
    RSF_CHECK_SLOT(session)
    RSF_CHECK_SLOT(backend)
    RSF_CHECK_SLOT(backend_choices)
    RSF_CHECK_SLOT(request)
    RSF_CHECK_SLOT(switch_result)
    RSF_CHECK_SLOT(generation)
    RSF_CHECK_SLOT(begin)
    RSF_CHECK_SLOT(marker)
    RSF_CHECK_SLOT(acquire)
    RSF_CHECK_SLOT(input)
    RSF_CHECK_SLOT(abort)
#undef RSF_CHECK_SLOT
    if (failures) return;

    // Pure atomic observations only. No graphics hook, setup or CPU marker is invoked.
    const auto owner = api.has_owner();
    const auto backend = api.backend();
    const auto choices = api.backend_choices();
    const auto switched = api.switch_result();
    const auto generation = api.generation();
    check(owner == 0, "successful query does not create a presentation owner");
    refusal_contract();
    check(api.has_owner() == owner && api.backend() == backend &&
          api.backend_choices() == choices && api.switch_result() == switched &&
          api.generation() == generation, "failed queries do not change bridge observations");

    struct extended_table {
        uint64_t before;
        rsf_d3d11_present_api table;
        unsigned char tail[64];
    } extended;
    std::memset(&extended, 0x5a, sizeof(extended));
    extended.table.struct_size = sizeof(extended) - offsetof(extended_table, table);
    extended.table.abi_version = RSF_D3D11_PRESENT_API_VERSION;
    check(rsf_get_d3d11_present_api(&extended.table) == RSF_OK, "larger caller capacity is accepted");
    check(extended.before == UINT64_C(0x5a5a5a5a5a5a5a5a), "query preserves leading canary");
    for (const auto byte : extended.tail) check(byte == 0x5a, "query preserves unknown trailing bytes");
    check(std::memcmp(&api, &extended.table, sizeof(api)) == 0, "query returns stable table bytes");

    // Queries only: concurrent population must not mutate settings or the returned table.
    std::atomic<bool> equal{true};
    std::array<std::thread, 8> threads;
    for (auto& thread : threads) {
        thread = std::thread([&] {
            for (unsigned i = 0; i < 1000; ++i) {
                rsf_d3d11_present_api copy{};
                copy.struct_size = sizeof(copy);
                copy.abi_version = RSF_D3D11_PRESENT_API_VERSION;
                if (rsf_get_d3d11_present_api(&copy) != RSF_OK ||
                    std::memcmp(&api, &copy, sizeof(copy)) != 0) equal.store(false);
            }
        });
    }
    for (auto& thread : threads) thread.join();
    check(equal.load(), "parallel queries return the same complete table");
}
}

int main() {
    refusal_contract();
    refusal_prefix_guard_page();
    populated_table_contract();
    if (failures) return 1;
    std::puts("PASS: real Runtime DLL import, complete bridge table, ABI refusal, canaries and concurrent queries");
    return 0;
}

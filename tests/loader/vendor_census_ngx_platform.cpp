// SPDX-License-Identifier: MIT
// NVSDK_NGX_Parameter overload leaf. MSVC assigns the virtual slot of each overload, which only a
// C++ compiler can derive from the SDK declaration order, so these calls cannot move to C or Rust.
#include <array>
#include <cstdint>
#include <utility>

#include "../vendor_census_ngx_params.h"
#include "ngx_param_probe.h"

namespace {

size_t g_slot = ~size_t{0};

template <size_t I>
int64_t stub(int64_t first)
{
    g_slot = I;
    return first;
}

template <size_t... I>
std::array<void*, sizeof...(I)> make_stubs(std::index_sequence<I...>)
{
    return {reinterpret_cast<void*>(&stub<I>)...};
}

const auto kStubs = make_stubs(std::make_index_sequence<48>{});

struct FakeObject {
    void* const* vtable;
};

NgxParam* fake_parameters()
{
    static FakeObject object{kStubs.data()};
    return reinterpret_cast<NgxParam*>(&object);
}

int g_failures = 0;

void expect_slot(size_t slot)
{
    if (g_slot != slot) ++g_failures;
}

} // namespace

extern "C" int rsf_test_ngx_check_slots(void)
{
    g_failures = 0;
    NgxParam* p = fake_parameters();
    // MSVC groups each overload set and reverses the declaration order.
    const auto slot = [&](size_t expected, auto&& call) {
        call();
        expect_slot(expected);
    };
    slot(0, [&] { p->Set("x", static_cast<void*>(nullptr)); });
    slot(1, [&] { p->Set("x", static_cast<ID3D12Resource*>(nullptr)); });
    slot(2, [&] { p->Set("x", static_cast<ID3D11Resource*>(nullptr)); });
    slot(3, [&] { p->Set("x", 0); });
    slot(4, [&] { p->Set("x", 0u); });
    slot(5, [&] { p->Set("x", 0.0); });
    slot(6, [&] { p->Set("x", 0.0f); });
    slot(7, [&] { p->Set("x", 0ull); });
    slot(8, [&] { p->Get("x", static_cast<void**>(nullptr)); });
    slot(9, [&] { p->Get("x", static_cast<ID3D12Resource**>(nullptr)); });
    slot(10, [&] { p->Get("x", static_cast<ID3D11Resource**>(nullptr)); });
    slot(11, [&] { p->Get("x", static_cast<int*>(nullptr)); });
    slot(12, [&] { p->Get("x", static_cast<unsigned int*>(nullptr)); });
    slot(13, [&] { p->Get("x", static_cast<double*>(nullptr)); });
    slot(14, [&] { p->Get("x", static_cast<float*>(nullptr)); });
    slot(15, [&] { p->Get("x", static_cast<unsigned long long*>(nullptr)); });
    slot(16, [&] { p->Reset(); });    return g_failures;
}

extern "C" void rsf_test_ngx_set_u32(void* parameters, const char* name, unsigned int value)
{
    static_cast<NgxParam*>(parameters)->Set(name, value);
}

extern "C" void rsf_test_ngx_set_i32(void* parameters, const char* name, int value)
{
    static_cast<NgxParam*>(parameters)->Set(name, value);
}

extern "C" void rsf_test_ngx_set_f32(void* parameters, const char* name, float value)
{
    static_cast<NgxParam*>(parameters)->Set(name, value);
}

extern "C" void rsf_test_ngx_set_resource12(void* parameters, const char* name, void* resource)
{
    static_cast<NgxParam*>(parameters)->Set(name, static_cast<ID3D12Resource*>(resource));
}

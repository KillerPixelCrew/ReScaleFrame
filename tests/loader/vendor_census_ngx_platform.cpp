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
    p->Set("x", static_cast<void*>(nullptr));
    expect_slot(0);
    p->Set("x", static_cast<ID3D12Resource*>(nullptr));
    expect_slot(1);
    p->Set("x", static_cast<ID3D11Resource*>(nullptr));
    expect_slot(2);
    p->Set("x", 0);
    expect_slot(3);
    p->Set("x", 0u);
    expect_slot(4);
    p->Set("x", 0.0);
    expect_slot(5);
    p->Set("x", 0.0f);
    expect_slot(6);
    p->Set("x", 0ull);
    expect_slot(7);
    p->Get("x", static_cast<void**>(nullptr));
    expect_slot(8);
    p->Get("x", static_cast<ID3D12Resource**>(nullptr));
    expect_slot(9);
    p->Get("x", static_cast<ID3D11Resource**>(nullptr));
    expect_slot(10);
    p->Get("x", static_cast<int*>(nullptr));
    expect_slot(11);
    p->Get("x", static_cast<unsigned int*>(nullptr));
    expect_slot(12);
    p->Get("x", static_cast<double*>(nullptr));
    expect_slot(13);
    p->Get("x", static_cast<float*>(nullptr));
    expect_slot(14);
    p->Get("x", static_cast<unsigned long long*>(nullptr));
    expect_slot(15);
    p->Reset();
    expect_slot(16);
    return g_failures;
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

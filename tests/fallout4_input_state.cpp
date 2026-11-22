// SPDX-License-Identifier: MIT
#include "../games/fallout4/src/native_input_state.h"
#include <cstddef>
#include <cstdio>
#include <cstring>

using rsf::fallout4::detail::NativeState;
extern "C" uint32_t rsf_fallout4_compare_input_state(const NativeState&, const NativeState&) noexcept;

namespace {
int failures{};
void check(bool value, const char* name) { if (!value) { ++failures; std::printf("FAIL %s\n",name); } }
void poison_padding(NativeState& value, unsigned char byte) {
    constexpr auto end=offsetof(NativeState,dynamic_viewport)+sizeof(value.dynamic_viewport);
    static_assert(sizeof(uintptr_t)==8 && end==122 && sizeof(NativeState)==128);
    auto* bytes=reinterpret_cast<unsigned char*>(&value);
    for (size_t i=end;i<sizeof(value);++i) bytes[i]=byte;
}
}
int main() {
    NativeState before{},after{};
    poison_padding(before,0xa5); poison_padding(after,0x5a);
    check(std::memcmp(&before,&after,sizeof(before))!=0,"old whole-object comparison observes poisoned padding");
    check(rsf_fallout4_compare_input_state(before,after)==3,"production comparisons ignore padding");
    if (!failures) std::puts("PASS Fallout4 snapshot padding");
    return failures ? 1 : 0;
}

// SPDX-License-Identifier: MIT
#include "../games/fallout4/src/native_input_state.h"
#include <cstddef>
#include <cstdio>
#include <cstring>

namespace {
using rsf::fallout4::detail::NativeState;
using rsf::fallout4::detail::same_native_owner;
using rsf::fallout4::detail::same_native_state;
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
    check(same_native_owner(before,after) && same_native_state(before,after),"padding is not an identity change");
    constexpr uintptr_t NativeState::* pointers[]{&NativeState::renderer,&NativeState::camera,&NativeState::cache,
        &NativeState::context,&NativeState::device,&NativeState::chain,&NativeState::window};
    for (auto member : pointers) {
        after=before; after.*member=1;
        check(!same_native_state(before,after),"each pointer field change refuses full state");
        const bool owner_field=member!=&NativeState::camera && member!=&NativeState::cache;
        check(same_native_owner(before,after)!=owner_field,"owner compares exactly its pointer fields");
    }
    constexpr uint32_t NativeState::* integers[]{&NativeState::frame,&NativeState::width,&NativeState::height,&NativeState::count,
        &NativeState::taa,&NativeState::disable,&NativeState::colour,&NativeState::motion,&NativeState::depth};
    for (auto member : integers) {
        after=before; after.*member=1;
        check(!same_native_state(before,after),"each integer field change refuses full state");
        const bool owner_field=member==&NativeState::frame || member==&NativeState::width || member==&NativeState::height;
        check(same_native_owner(before,after)!=owner_field,"owner compares exactly its integer fields");
    }
    for (unsigned i=0;i<2;++i) {
        after=before; after.ratio[i]=1;
        check(!same_native_state(before,after) && same_native_owner(before,after),"each ratio element remains checked only by full state");
    }
    for (unsigned i=0;i<4;++i) {
        after=before; after.viewport[i]=1;
        check(!same_native_state(before,after) && same_native_owner(before,after),"each viewport element remains checked only by full state");
    }
    after=before; after.delta_seconds=1;
    check(same_native_state(before,after) && same_native_owner(before,after),"timing updates do not invalidate unchanged render inputs");
    after=before; after.default_viewport=1;
    check(!same_native_state(before,after) && same_native_owner(before,after),"default selector remains checked only by full state");
    after=before; after.dynamic_viewport=1;
    check(!same_native_state(before,after) && same_native_owner(before,after),"dynamic selector remains checked only by full state");
    after=before; after.ratio[0]=-0.0f;
    check(!same_native_state(before,after),"signed zero retains exact field-bit comparison");
    before.ratio[0]=std::bit_cast<float>(uint32_t(0x7fc00123)); after=before;
    check(same_native_state(before,after),"identical NaN field bits retain prior comparison semantics");
    after.ratio[0]=std::bit_cast<float>(uint32_t(0x7fc00124));
    check(!same_native_state(before,after),"different float field bits refuse");
    if (!failures) std::puts("PASS Fallout4 snapshot padding and semantic-field comparisons");
    return failures ? 1 : 0;
}

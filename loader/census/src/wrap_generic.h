// SPDX-License-Identifier: GPL-3.0-only
// Wrapper templates for exports whose argument counts are known only loosely. A generic wrapper
// takes the eight integer argument registers and stack slots of the Windows x64 convention and
// forwards all of them, so a callee of up to eight integer or pointer arguments sees exactly what
// the caller passed. It cannot forward a floating point argument or return value; those functions
// get an exact signature instead.
#pragma once

#include <intrin.h>

#include "census_internal.h"

namespace rsf_census {

using Q = uint64_t;
using Decoder = void (*)(Json&, const Q*);
using PostDecoder = void (*)(Json&, const Q*, Q);
// Runs after every call, recorded or not: bookkeeping that must not depend on sampling.
using Action = void (*)(const Q*, Q);
using BeforeAction = void (*)(const Q*);

template <Fn F, Decoder Pre = nullptr, PostDecoder Post = nullptr, Action Act = nullptr, BeforeAction Begin = nullptr>
Q gen8(Q a0, Q a1, Q a2, Q a3, Q a4, Q a5, Q a6, Q a7) noexcept {
    using Target = Q (*)(Q, Q, Q, Q, Q, Q, Q, Q);
    auto o = reinterpret_cast<Target>(orig<F>());
    Call c(F, _ReturnAddress());
    const Q a[8] = {a0, a1, a2, a3, a4, a5, a6, a7};
    if constexpr (Pre != nullptr) {
        if (c.full()) Pre(c.json(), a);
    }
    if constexpr (Begin != nullptr) Begin(a);
    const Q r = o ? o(a0, a1, a2, a3, a4, a5, a6, a7) : 0;
    if constexpr (Post != nullptr) {
        if (c.full()) Post(c.json(), a, r);
    }
    if constexpr (Act != nullptr) Act(a, r);
    c.finish(r);
    return r;
}

#define RSF_REG(t, name, ...) t[idx(Fn::name)] = reinterpret_cast<void*>(&gen8<Fn::name, ##__VA_ARGS__>)

inline void put_raw_args(Json& j, const Q* a, size_t n) noexcept {
    j.arr("raw_args");
    for (size_t i = 0; i < n; ++i) j.hex(nullptr, a[i]);
    j.end_arr();
}

// Report the pointer an out parameter points at after the call.
inline void put_out_ptr(Json& j, const char* key, Q out) noexcept {
    void* v = nullptr;
    if (out && plausible_ptr(reinterpret_cast<void*>(out)) && rd(reinterpret_cast<void*>(out), v)) {
        j.hex(key, reinterpret_cast<uint64_t>(v));
    } else {
        j.null(key);
    }
}

} // namespace rsf_census

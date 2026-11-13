// SPDX-License-Identifier: GPL-3.0-only
#include "sr_reconstruction.h"
#include "sr_jitter.h"
#include <cstdio>
#include <cstring>
#include <limits>

using namespace rsf::anomaly::sr;
namespace {
int failures{};
#define CHECK(x) do { if (!(x)) { std::printf("FAIL %d: %s\n", __LINE__, #x); ++failures; } } while (false)
template<class T, std::size_t N> void put(std::array<std::byte, N>& a, std::size_t at, const T& value) {
    std::memcpy(a.data() + at, &value, sizeof(value));
}
void numeric_round_trip() {
    for (const auto size : {std::array<uint32_t, 2>{853, 481}, {1286, 724}, {1920, 1080}}) {
        for (const float fov : {0.55f, 1.1f, 1.7f}) {
            const float v = -std::tan(fov * 0.5f), h = -v * float(size[0]) / float(size[1]);
            EngineMatrix projection{1.0f/h,0,0,0, 0,-1.0f/v,0,0, 0,0,1.0001f,1, 0,0,-0.1f,0};
            for (uint32_t phase = 0; phase < 18; ++phase) {
                const auto sample = jitter_sample(phase, 18);
                const auto jittered = jitter_engine_projection(projection, sample.x, sample.y, size[0], size[1]);
                CHECK(jittered.has_value());
                if (!jittered) continue;
                PositionVector stock{h, v, 2*h/float(size[0]), 2*v/float(size[1])}, corrected = stock;
                CHECK(correct_position_vector(corrected, sample.x, sample.y));
                CHECK(corrected[2] == stock[2] && corrected[3] == stock[3]);
                for (const float z : {0.5f, 5.0f, 40.0f, 200.0f}) {
                    const float x = z*0.25f, y = z*-0.15f;
                    const float px = ((x*(*jittered)[0] + z*(*jittered)[8])/z + 1)*float(size[0])*0.5f;
                    const float py = (1 - (y*(*jittered)[5] + z*(*jittered)[9])/z)*float(size[1])*0.5f;
                    const float reconstructed_x = z*(px*corrected[2]-corrected[0]);
                    const float reconstructed_y = z*(py*corrected[3]-corrected[1]);
                    CHECK(std::abs(reconstructed_x-x) <= 2e-6f*(1+std::abs(x)));
                    CHECK(std::abs(reconstructed_y-y) <= 2e-6f*(1+std::abs(y)));
                    const float stock_error_x = z*(px*stock[2]-stock[0])-x;
                    CHECK(std::abs(stock_error_x - z*sample.x*stock[2]) <= 2e-6f*(1+std::abs(x)));
                }
            }
        }
    }
}
struct Engine final : PositionConstantHost {
    std::array<std::byte, 0x40> constant{};
    std::array<std::array<std::byte, 0x80>, 6> cb{};
    std::array<std::array<std::byte, 128>, 6> data{};
    std::array<std::byte*, 6> selected{};
    int writes{}, fail_write{-1}, gets{};
    static constexpr PositionVector stock{0.8f, -0.5f, 0.001f, -0.002f};
    Engine() {
        const uint32_t destinations = 0x7b;
        put(constant, 0x10, destinations);
        for (uint32_t i = 0; i < 6; ++i) {
            const uint16_t index = uint16_t(16 + i*16), cls = 1;
            put(constant, 0x14 + i*4, index); put(constant, 0x16 + i*4, cls);
            const uint32_t bytes = 128; put(cb[i], 0x58, bytes);
            auto* ptr = data[i].data(); put(cb[i], 0x60, ptr);
            put(data[i], index, stock); selected[i] = cb[i].data();
        }
    }
    bool read(const void* p, void* out, std::size_t n) noexcept override { if (!p) return false; std::memcpy(out,p,n); return true; }
    bool write(void* p, const void* in, std::size_t n) noexcept override {
        if (writes++ == fail_write) return false;
        std::memcpy(p,in,n); return true;
    }
    std::byte* buffer(void* c, uint32_t stage) noexcept override { CHECK(c == constant.data()); ++gets; return selected[stage]; }
    PositionVector value(uint32_t stage) const {
        PositionVector out{}; std::memcpy(out.data(),data[stage].data()+16+stage*16,sizeof(out)); return out;
    }
};
void staging_contract() {
    Engine e;
    CHECK(correct_position_constant(e,e.constant.data(),0.25f,-0.125f));
    CHECK(e.gets == 6 && e.writes == 12);
    for (uint32_t i = 0; i < 6; ++i) {
        const auto v=e.value(i);
        CHECK(v[0] == 0.8f + 0.25f*0.001f && v[1] == -0.5f + (-0.125f)*(-0.002f));
        CHECK(v[2] == Engine::stock[2] && v[3] == Engine::stock[3]);
        CHECK(e.cb[i][0x68] == std::byte{1});
    }
    for (uint32_t stage = 0; stage < 6; ++stage) {
        Engine single; put(single.constant,0x10,constant_stage_bits[stage]);
        CHECK(correct_position_constant(single,single.constant.data(),0.25f,-0.125f));
        CHECK(single.gets == 1 && single.writes == 2);
        for (uint32_t other = 0; other < 6; ++other) if (other != stage) CHECK(single.value(other) == Engine::stock);
    }
    Engine alias; alias.selected[1]=alias.selected[0]; const uint16_t index=16; put(alias.constant,0x18,index);
    CHECK(correct_position_constant(alias,alias.constant.data(),0.25f,-0.125f));
    CHECK(alias.writes == 10 && alias.value(0)[0] == 0.8f+0.25f*0.001f);
    for (int failure=0; failure < 12; ++failure) {
        Engine bad; const auto data=bad.data, cb=bad.cb; bad.fail_write=failure;
        CHECK(!correct_position_constant(bad,bad.constant.data(),0.25f,-0.125f));
        CHECK(bad.data == data && bad.cb == cb);
    }
    for (int failure=0; failure < 5; ++failure) {
        Engine bad;
        if (failure==0) { const uint16_t cls=3; put(bad.constant,0x16+5*4,cls); }
        if (failure==1) { const uint32_t small=16; put(bad.cb[5],0x58,small); }
        if (failure==2) { const uint32_t invalid=0x7b | (15u<<16); put(bad.constant,0x10,invalid); }
        if (failure==3) { const uint32_t sampler=4; put(bad.constant,0x10,sampler); }
        if (failure==4) { auto invalid=Engine::stock; invalid[0]=std::numeric_limits<float>::quiet_NaN(); put(bad.data[5],96,invalid); }
        CHECK(!correct_position_constant(bad,bad.constant.data(),0.25f,-0.125f));
        CHECK(bad.writes == 0);
    }
}
}
int main() { numeric_round_trip(); staging_contract(); std::printf("%d failure(s)\n",failures); return failures ? 1 : 0; }

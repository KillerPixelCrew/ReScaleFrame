// SPDX-License-Identifier: GPL-3.0-only
#include "latency_antilag.h"
#include "latency_policy.h"
#include <cstdio>
using namespace rsf::latency;
namespace {
int failures{};
#define CHECK(x) do { if (!(x)) { std::printf("FAIL %d: %s\n",__LINE__,#x); ++failures; } } while(false)
struct Driver : AntiLagDriver {
    int initializes{},updates{},ends{},shutdowns{}; uint32_t api{},fps{}; bool enabled{},generated{},ready{true},update_ok{true};
    bool initialize(uint32_t a,void*) noexcept override { ++initializes; api=a; return ready; }
    bool update(bool e,uint32_t f) noexcept override { ++updates; enabled=e; fps=f; return update_ok; }
    bool end_render(void*,bool e,bool g) noexcept override { ++ends; enabled=e; generated=g; return true; }
    void shutdown() noexcept override { ++shutdowns; }
};
}
int main() {
    CHECK(pacing_owner(RSF_PACING_XELL,true,true)==RSF_PACING_XELL);
    CHECK(pacing_owner(RSF_PACING_REFLEX,false,true)==RSF_PACING_REFLEX);
    CHECK(pacing_owner(RSF_PACING_NONE,true,true)==RSF_PACING_REFLEX);
    CHECK(pacing_owner(RSF_PACING_NONE,false,true)==RSF_PACING_ANTILAG2);
    CHECK(pacing_owner(RSF_PACING_NONE,false,false)==RSF_PACING_NONE);
    Driver d; AntiLag owner(d); int device{},chain{};
    owner.bind(12,&device,0x10de); CHECK(!owner.state().available && d.initializes==0);
    owner.bind(11,&device,0x8086); CHECK(!owner.pace(1,1,0,0) && d.updates==0);
    owner.bind(12,&device,0x1002); CHECK(owner.state().available && d.initializes==1 && d.api==12);
    CHECK(owner.pace(1,1,10000,0) && d.enabled && d.fps==100);
    CHECK(!owner.pace(1,1,10000,0) && d.updates==1); // one pacing update per source
    CHECK(owner.end_render(1,&chain,true) && d.ends==1 && d.enabled && d.generated);
    CHECK(!owner.end_render(1,&chain,true) && d.ends==1);
    CHECK(!owner.pace(2,0,0,0) && !d.enabled && d.fps==0);
    CHECK(!owner.pace(3,1,0,RSF_PACING_XELL) && !d.enabled); // never a second independent sleeper
    CHECK(!owner.pace(4,2,0,RSF_PACING_REFLEX) && !d.enabled);
    owner.bind(11,&device,0x1002); CHECK(d.api==11 && owner.pace(5,2,20000,0) && d.fps==50);
    CHECK(!owner.end_render(5,&chain,true)); // DX11 has no DX12 FSR typing interface
    d.update_ok=false; CHECK(!owner.pace(6,1,0,0) && !owner.state().available && owner.state().refused==1);
    const int updates=d.updates; CHECK(!owner.pace(7,1,0,0) && d.updates==updates);
    Driver unsupported; unsupported.ready=false; AntiLag missing(unsupported); missing.bind(12,&device,0x1002);
    CHECK(!missing.state().available && missing.state().refused==1 && !missing.pace(1,1,0,0));
    std::printf("%d failures\n",failures); return failures ? 1 : 0;
}

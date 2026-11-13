// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/native_cpu.h>
#include <cstdio>
#include <vector>
namespace {
int failures{};
#define CHECK(x) do { if (!(x)) { std::printf("FAIL %d: %s\n",__LINE__,#x); ++failures; } } while(false)
std::vector<rsf_game_cpu_event> delivered;
void sink(void*,const rsf_game_cpu_event* event) { delivered.push_back(*event); }
rsf_game_cpu_event fact(uint64_t source,uint32_t stage,uint64_t stamp) {
    return {sizeof(rsf_game_cpu_event),stage,9,source,stamp,1000,1,0,0,RSF_GAME_CPU_EVENT_VERSION,RSF_GAME_CPU_INPUT_KEYED};
}
int send(rsf_game_cpu_event event) { return rsf_native_cpu_dispatch(&event,&sink,nullptr); }
}
int main() {
    rsf_native_cpu_release();
    CHECK(send(fact(1,RSF_GAME_CPU_FRAME_BEGIN,10)));
    CHECK(send(fact(1,RSF_GAME_CPU_INPUT_SAMPLE,20)));
    CHECK(send(fact(1,RSF_GAME_CPU_SIMULATION_BEGIN,21)));
    const auto count=delivered.size();
    CHECK(send(fact(1,RSF_GAME_CPU_INPUT_SAMPLE,22)) && delivered.size()==count);
    CHECK(send(fact(1,RSF_GAME_CPU_SIMULATION_BEGIN,22)) && delivered.size()==count);
    for (uint32_t kind : {1u,1u,2u,4u,7u}) { auto e=fact(1,RSF_GAME_CPU_INPUT_EVENT,23); e.input_kind=kind; CHECK(send(e)); }
    rsf_native_cpu_ping_message(0xc123);
    auto ping=fact(1,RSF_GAME_CPU_INPUT_EVENT,24); ping.message_id=0xc123;
    CHECK(send(ping)); const auto after_ping=delivered.size(); CHECK(send(ping) && delivered.size()==after_ping);
    CHECK(send(fact(1,RSF_GAME_CPU_SIMULATION_END,30)));
    for(uint32_t marker=0;marker<6;++marker) CHECK(rsf_native_cpu_marker(1,marker)==1);
    CHECK(rsf_native_cpu_marker(1,5)==0);
    CHECK(send(fact(1,RSF_GAME_CPU_FRAME_END,40)));
    CHECK(!send(ping)); // no stale event may leak after ownership closed
    rsf_native_cpu_frame frame{}; frame.struct_size=sizeof(frame);
    CHECK(rsf_native_cpu_read(9,1,&frame));
    CHECK(frame.input_mask==7 && frame.input_events[0]==1 && frame.input_events[1]==1 && frame.input_events[2]==1);
    CHECK(frame.pings==1 && frame.marker_mask==63);
    delivered.clear(); CHECK(send(fact(2,RSF_GAME_CPU_FRAME_BEGIN,50)));
    CHECK(send(fact(2,RSF_GAME_CPU_SIMULATION_END,60)));
    CHECK(delivered.size()==4 && delivered[1].stage==RSF_GAME_CPU_INPUT_SAMPLE && delivered[2].stage==RSF_GAME_CPU_SIMULATION_BEGIN);
    CHECK((delivered[1].flags & RSF_GAME_CPU_INPUT_SUBSTITUTE) && (delivered[2].flags & RSF_GAME_CPU_INPUT_SUBSTITUTE));
    CHECK(rsf_native_cpu_marker(2,2)==-1);
    rsf_native_cpu_counters stats{}; stats.struct_size=sizeof(stats); CHECK(rsf_native_cpu_counters_get(&stats));
    CHECK(stats.frames==2 && stats.pings==1 && stats.substitutes==1 && stats.order_violations==1);
    CHECK(stats.input_events[0]==1 && stats.input_events[1]==1 && stats.input_events[2]==1 && stats.duplicates>=5);
    CHECK(send(fact(2,RSF_GAME_CPU_FRAME_END,70)));
    auto message=fact(0,RSF_GAME_CPU_INPUT_EVENT,75); message.message_id=0xc123;
    message.flags |= RSF_GAME_CPU_WINDOW_MESSAGE;
    CHECK(send(message)); delivered.clear();
    CHECK(send(fact(3,RSF_GAME_CPU_FRAME_BEGIN,80)));
    CHECK(delivered.size()==2 && delivered[1].source_frame_id==3 && delivered[1].message_id==0xc123);
    CHECK(send(fact(3,RSF_GAME_CPU_FRAME_END,90)));
    CHECK(send(message)); delivered.clear();
    CHECK(send(fact(4,RSF_GAME_CPU_FRAME_BEGIN,1000)) && delivered.size()==1);
    CHECK(rsf_native_cpu_counters_get(&stats) && stats.expired_messages==1 && stats.pings==2);
    CHECK(send(fact(4,RSF_GAME_CPU_FRAME_END,1010)));
    CHECK(send(fact(5,RSF_GAME_CPU_FRAME_BEGIN,1100)));
    CHECK(send(fact(5,RSF_GAME_CPU_SIMULATION_BEGIN,1110))); // real engine update can start before its poll
    CHECK(send(fact(5,RSF_GAME_CPU_INPUT_SAMPLE,1120)));
    CHECK(send(fact(5,RSF_GAME_CPU_SIMULATION_END,1130)));
    CHECK(rsf_native_cpu_counters_get(&stats) && stats.order_violations==1 && stats.substitutes==1);
    auto invalid=fact(6,RSF_GAME_CPU_FRAME_BEGIN,1200); invalid.event_version=99; CHECK(!send(invalid));
    invalid.event_version=1; invalid.flags=0x80000000; CHECK(!send(invalid));
    auto legacy=fact(6,RSF_GAME_CPU_FRAME_BEGIN,1300); legacy.event_version=legacy.flags=0;
    CHECK(send(legacy));
    CHECK(rsf_native_cpu_marker(6,4)==-1); // even a threaded Present needs a render submission
    for (uint32_t marker : {0u,2u,4u,5u,1u,3u}) CHECK(rsf_native_cpu_marker(6,marker)==1);
    CHECK(rsf_native_cpu_counters_get(&stats) && stats.order_violations==2);
    std::printf("%d failures\n",failures); return failures ? 1 : 0;
}

// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/native_cpu.h>
#include "../runtime/orchestrator/src/input_receipts.h"
#include <cstdio>
#include <cstring>
namespace { int errors{}, delivered{};
void check(bool okay) { if (!okay) ++errors; }
void sink(void*,const rsf_game_cpu_event* event) {
    check(event->source_frame_id==0 && event->event_version==RSF_GAME_CPU_EVENT_VERSION_RECEIPT);
    check(event->struct_size == sizeof(rsf_game_cpu_receipt_event));
    ++delivered;
}
}
int main() {
    static_assert(sizeof(rsf_game_cpu_receipt_event)==80);
    rsf_native_cpu_release();
    rsf_game_cpu_event begin{sizeof(begin),RSF_GAME_CPU_FRAME_BEGIN,7,1,1,1000,0,0,0,1,RSF_GAME_CPU_INPUT_KEYED};
    check(rsf_native_cpu_event(&begin)==1);
    struct ExtendedReceipt { rsf_game_cpu_receipt_event packet; uint64_t tail; };
    ExtendedReceipt extended{};
    auto& event = extended.packet;
    event.header={sizeof(event),RSF_GAME_CPU_INPUT_EVENT,7,0,2,1000,0,RSF_GAME_INPUT_MOUSE,0,
        RSF_GAME_CPU_EVENT_VERSION_RECEIPT,RSF_GAME_CPU_INPUT_UNFRAMED};
    event.receipt_id=2; event.window_key=123;
    event.header.struct_size = sizeof(extended);
    check(rsf_native_cpu_dispatch(&event.header,sink,nullptr)==1);
    event.header.struct_size = sizeof(event);
    event.receipt_id=1; check(rsf_native_cpu_dispatch(&event.header,sink,nullptr)==1);
    check(rsf_native_cpu_dispatch(&event.header,sink,nullptr)==0);
    event.receipt_id=3;event.header.input_kind=8;check(rsf_native_cpu_dispatch(&event.header,sink,nullptr)==0);
    event.header.input_kind=RSF_GAME_INPUT_MOUSE;check(rsf_native_cpu_dispatch(&event.header,sink,nullptr)==1);
    event.header.struct_size=sizeof(rsf_game_cpu_event);check(rsf_native_cpu_dispatch(&event.header,sink,nullptr)==0);
    rsf_native_input_receipts counts{sizeof(counts),1};check(rsf_native_input_receipts_get(&counts)==1);
    check(counts.accepted==3 && counts.duplicates==1 && counts.rejected==1 && counts.input_kinds[1]==3 && delivered==3);
    rsf_native_cpu_frame frame{sizeof(frame)};check(rsf_native_cpu_read(7,1,&frame)==1);
    check(frame.input_mask==0 && frame.pings==0 && frame.stage_mask==1 && frame.marker_mask==0);
    rsf::latency::ReceiptWindow window;
    check(window.accept(100));check(window.accept(99));check(!window.accept(99));check(!window.accept(36));
    check(window.accept(200));check(!window.accept(100));check(!window.accept(0));
    rsf::latency::ReceiptOwner owners[]{{10,7,2,100,110,true},{11,7,2,101,111,false}};
    check(rsf::latency::receipt_owner(owners,7,2,120)==10);
    check(rsf::latency::receipt_owner(owners,7,2,105)==0); // cannot route before actual simulation
    check(rsf::latency::receipt_owner(owners,7,3,120)==0); // different provider generation
    check(rsf::latency::receipt_owner(owners,8,2,120)==0); // foreign session
    owners[1].eligible=true;
    check(rsf::latency::receipt_owner(owners,7,2,120)==0); // ambiguity must not pick latest
    owners[0].eligible=false;
    check(rsf::latency::receipt_owner(owners,7,2,120)==11);
    rsf_native_cpu_release();check(rsf_native_input_receipts_get(&counts)==1 && counts.accepted==0);
    std::printf("input receipts: %d failures\n",errors);return errors ? 1 : 0;
}

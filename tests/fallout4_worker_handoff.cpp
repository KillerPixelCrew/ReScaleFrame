// SPDX-License-Identifier: GPL-3.0-only
#include "../games/fallout4/src/worker_handoff.h"
#include <atomic>
#include <cstdio>
#include <initializer_list>
#include <latch>
#include <mutex>
#include <thread>

namespace {
using rsf::fallout4::CpuIdentity;
using rsf::fallout4::detail::WorkerCycle;
using rsf::fallout4::detail::WorkerTicket;
std::atomic<unsigned> failures{};
void check(bool value, const char* name) {
    if (!value) { failures.fetch_add(1); std::printf("FAIL %s\n",name); }
}
WorkerCycle predecessor() { return {7,40,20,1,2,3,4,5,6,7,8,100}; }
CpuIdentity successor() { return {7,41,0,0,10000000}; }
WorkerTicket consumed_ticket() {
    WorkerTicket ticket;
    check(ticket.reserve(predecessor(),successor(),9,true),"reserve after predecessor allocation/Present entry");
    check(ticket.arm(predecessor(),9,12,true),"raw revision snapped after RESERVE return");
    check(ticket.sample(9,12,77,1000,true),"actual poll completed");
    check(ticket.begin(1010,9,12,77,true),"actual receiver begins future flags0 frame");
    check(ticket.bind(9,12,77,true),"strict matching sample binds at receiver");
    check(ticket.consumed(9,77,true),"native receiver returns with matching consumed cursor");
    return ticket;
}
WorkerTicket joined_ticket() {
    auto ticket=consumed_ticket();
    check(ticket.complete(true),"selected worker callback returns");
    check(ticket.join(predecessor(),true),"verified actual wait completed");
    return ticket;
}
WorkerCycle next_swap() {
    auto owner=predecessor(); owner.source=41; ++owner.idle; return owner;
}
}
int main() {
    // The same unmatched generation refuses a future ticket in both cases;
    // only the current renderer's claim to sampled input changes the outcome.
    for (const uint64_t bound_source : {uint64_t(0),uint64_t(40)}) {
        auto future=joined_ticket();
        future.refuse(RSF_GAME_CPU_ABORT_INTERVENING_POLL);
        const bool invalid=rsf::fallout4::detail::invalidates_render_source(bound_source,true);
        check(invalid==(bound_source!=0),"pure unmatched future poll preserves only input-unbound renderer");
        check(!future.claim(next_swap(),77,9) && future.frame_begun && future.refusal,
            "preserved renderer never revives the refused future or drops abort/end obligations");
        WorkerTicket early;
        check(!early.reserve(predecessor(),successor(),10,false),"unbound preservation never admits an early future");
        // These are explicitly not poll-only facts in the production callsite:
        // quiesce, active raw writer/overlap, and native owner/state mismatch.
        for (const uint32_t reason : {RSF_GAME_CPU_ABORT_QUIESCE,RSF_GAME_CPU_ABORT_OVERLAP,RSF_GAME_CPU_ABORT_STATE_CHANGED}) {
            auto refused=consumed_ticket(); refused.refuse(reason);
            check(rsf::fallout4::detail::invalidates_render_source(bound_source,false),
                "lifecycle/raw/native refusal invalidates bound and unbound renderer");
            check(refused.refusal==reason && !refused.complete(true),"native refusal remains sticky on future ticket");
        }
    }
    WorkerTicket empty;
    check(!empty.reserve(predecessor(),successor(),9,false),"early/straddling poll cannot reserve before Present entry");
    auto bad_source=successor(); bad_source.source=40;
    check(!empty.reserve(predecessor(),bad_source,9,true),"future id must follow allocated predecessor");
    auto unknown=predecessor(); unknown.source=0;
    check(!empty.reserve(unknown,successor(),9,true),"unknown predecessor refuses");

    WorkerTicket pending;
    check(pending.reserve(predecessor(),successor(),9,true),"pending fixture reserve");
    check(pending.arm(predecessor(),9,12,true),"post-wait raw revision may differ from pre-reserve raw revision");
    check(!pending.sample(10,12,77,1000,true),"intervening poll before sample refuses");
    check(!pending.sample(9,13,77,1000,true),"overlapped raw sample refuses");
    check(pending.sample(9,12,77,1000,true),"unchanged actual sample succeeds");
    check(!pending.begin(1010,9,13,77,true),"raw change before BIND refuses");
    check(!pending.begin(1010,9,12,78,true),"queue advance without matching sampled cursor refuses");
    check(!pending.begin(1010,9,12,77,false),"native state/receiver mismatch refuses");

    auto consumed=consumed_ticket();
    check(!consumed.complete(false),"changed worker/native state cannot complete");
    // Consumed sources do not compare later raw revisions. Future adoption still
    // requires their exact consumed cursor and absence of an unmatched poll.
    check(consumed.complete(true),"later raw arrival does not rewrite genuinely consumed source");
    check(!consumed.join(predecessor(),false),"early/no-op join EAX0 is not actual completed wait");
    auto changed=predecessor(); ++changed.complete_event;
    check(!consumed.join(changed,true),"changed native completion handle refuses join");
    check(consumed.join(predecessor(),true),"actual matching wait closes worker handoff");
    check(!consumed.claim(next_swap(),78,9),"new queue cursor cannot be relabeled as consumed ticket");
    check(!consumed.claim(next_swap(),77,10),"unmatched later poll invalidates successor");
    changed=next_swap(); ++changed.idle;
    check(!consumed.claim(changed,77,9),"no immediate successor refuses delayed ticket");
    changed=next_swap(); ++changed.frame;
    check(!consumed.claim(changed,77,9),"Swap must precede next native frame increment");
    check(consumed.claim(next_swap(),77,9),"next Swap claims already allocated id without second FRAME_BEGIN");
    check(!consumed.adopt(next_swap(),77,9),"World requires actual native frame increment");
    changed=next_swap(); ++changed.frame;
    check(consumed.adopt(changed,77,9),"immediate ordinary World native+1 adopts consumed source");
    check(!consumed.adopt(changed,77,9),"duplicate world cannot adopt same ticket twice");

    auto stopped=joined_ticket(); stopped.refuse(1); stopped.refuse(2);
    check(stopped.refusal==1 && stopped.active() && stopped.frame_begun,"quiesce retains sticky abort/frame-end obligations");
    check(!stopped.claim(next_swap(),77,9),"quiesced ticket cannot be replaced with fallback ahead of abort drain");

    // Exercise the real scalar helper across the producer/graphics handoff. The
    // simulated blocking host callback must release the adapter's scalar lock;
    // Present and the original native join can progress while it is blocked.
    std::mutex guard;
    std::latch reserve_entered(1),present_returned(1);
    WorkerTicket cross;
    std::thread worker([&] {
        { std::lock_guard lock(guard); check(cross.reserve(predecessor(),successor(),9,true),"worker reserves with predecessor phase open"); }
        reserve_entered.count_down(); present_returned.wait();
        { std::lock_guard lock(guard);
            check(cross.arm(predecessor(),9,20,true),"RESERVE returns after Present exit; retained phase proof suffices");
            check(cross.sample(9,20,77,1000,true),"worker actual poll after core wait");
            check(cross.begin(1010,9,20,77,true) && cross.bind(9,20,77,true),"actual receiver FRAME_BEGIN/BIND");
            check(cross.consumed(9,77,true) && cross.complete(true),"native worker consumed source completion");
        }
    });
    reserve_entered.wait();
    const bool unlocked=guard.try_lock(); check(unlocked,"no scalar lock held across blocking host callback");
    if (unlocked) guard.unlock();
    present_returned.count_down(); worker.join();
    { std::lock_guard lock(guard);
        check(cross.join(predecessor(),true) && cross.claim(next_swap(),77,9),"post-Swap join transfers ticket to next graphics Swap");
        changed=next_swap(); ++changed.frame;
        check(cross.adopt(changed,77,9),"cross-thread handoff preserves actual native+1 source association");
    }
    if (!failures.load()) std::puts("PASS Fallout4 bounded worker successor handoff");
    return failures.load() ? 1 : 0;
}

// SPDX-License-Identifier: GPL-3.0-only
#include "../games/skyrim-se/src/cpu_events.h"
#include "../games/skyrim-se/src/input_ownership.h"
#include "../games/skyrim-se/src/screen_classification.h"
#include <windows.h>
#include <array>
#include <cstdio>

namespace {
struct Event { uintptr_t vtable{}; uint32_t device{}, type{}; Event* next{}; };
int failures{};
void check(bool passed, const char* label) {
    if (!passed) { ++failures; std::printf("FAIL %s\n", label); }
}
}
int main() {
    rsf::skyrim::NativeJobOwnership jobs;
    check(!jobs.snapshot().generation, "no native submission has no job ownership");
    check(!jobs.publish(0, 11, 22).generation, "null manager refuses publication");
    const auto first_job = jobs.publish(0x1234, 11, 22);
    check(first_job.generation && jobs.matches(first_job), "pre-wakeup ticket publishes immutable submission");
    auto changed_job = first_job; changed_job.source = 99;
    check(!jobs.matches(changed_job) && !jobs.retire(changed_job), "different source cannot retire native ticket");
    check(jobs.retire(first_job) && !jobs.snapshot().generation, "completed native join retires matching generation");
    const auto next_job = jobs.publish(0x1234, 12, 22);
    check(next_job.generation > first_job.generation && !jobs.retire(first_job), "stale join cannot retire next generation");
    check(!jobs.publish(0x1234, 13, 22).generation && !jobs.snapshot().generation, "overlapping submission poisons provenance");
    check(!jobs.publish(0x1234, 14, 22).generation, "poisoned submission refuses reuse until quiescent reset");
    jobs.reset();
    const auto restarted_job = jobs.publish(0x1234, 15, 23);
    check(jobs.matches(restarted_job), "quiescent restart establishes a fresh session ticket");
    jobs.invalidate();
    check(!jobs.matches(restarted_job) && !jobs.retire(restarted_job), "quiesce rejects outstanding job ticket");
    check(!jobs.retired(), "poisoned provenance has no joined generation");
    jobs.reset();
    const auto joined_job = jobs.publish(0x1234, 16, 23);
    check(jobs.retire(joined_job) && jobs.retired() == joined_job.generation, "normal join records its generation");

    // Prospective source: render S=16 overlaps the late poll that reserves P=17.
    using Sources = rsf::skyrim::ProspectiveSources;
    Sources sources;
    auto reserved = sources.reserve(17, 16, 23, joined_job.generation, 5);
    check(reserved.reserved.source == 17 && !reserved.withdrawn, "verified mode1 poll reserves a future source");
    check(!sources.adopt(23, joined_job.generation).source, "poll in flight cannot be adopted");
    check(sources.still_reserved(17) && sources.sample(17, true, RSF_GAME_INPUT_MOUSE), "completed poll samples its reservation");
    check(!sources.adopt(24, joined_job.generation).source, "stale session cannot adopt");
    check(!sources.adopt(23, joined_job.generation + 4).source, "unjoined job generation cannot adopt");
    const auto adopted = sources.adopt(23, joined_job.generation);
    check(adopted.source == 17 && adopted.predecessor == 16 && adopted.phase == Sources::adopted,
        "next tick adopts the sampled future identity");
    check(!sources.bind(17, 6), "an intervening completed poll refuses binding");
    check(sources.bind(17, 5) && sources.owned().phase == Sources::bound, "unchanged control state binds at submit");
    check(!sources.bind(17, 5), "one binding per reservation");
    // The owning tick's own late poll overlaps its update: a separate pending reservation.
    reserved = sources.reserve(18, 17, 23, joined_job.generation + 4, 6);
    check(reserved.reserved.source == 18 && !reserved.withdrawn && sources.owned().source == 17,
        "next late poll does not disturb the bound owner");
    check(sources.release(17).phase == Sources::bound && !sources.owned().source, "tick end releases its own reservation");
    // Overlapping polls: a second reservation while one is still in flight withdraws both.
    reserved = sources.reserve(19, 17, 23, joined_job.generation + 4, 7);
    check(!reserved.reserved.source && reserved.withdrawn == 18 && reserved.overlap, "overlapping polls own nothing");
    // Intervening poll: a newer sampled reservation replaces an unadopted one.
    (void)sources.reserve(20, 17, 23, joined_job.generation + 4, 8);
    (void)sources.sample(20, true, 0);
    reserved = sources.reserve(21, 17, 23, joined_job.generation + 4, 9);
    check(reserved.reserved.source == 21 && reserved.withdrawn == 20 && !reserved.overlap, "newer poll withdraws sampled reservation");
    check(sources.withdraw_pending() == 21 && !sources.withdraw_pending(), "mode2 or paused poll withdraws pending reservation");
    (void)sources.reserve(22, 17, 23, joined_job.generation + 4, 10);
    check(!sources.withdraw_pending(23) && sources.withdraw_pending(22) == 22, "targeted withdrawal only removes its own reservation");
    (void)sources.reserve(24, 17, 23, joined_job.generation + 4, 11);
    (void)sources.sample(24, false, 0);
    check(!sources.adopt(23, joined_job.generation + 4).source, "poll without produced control state is never adopted");
    sources.reset();
    check(!sources.withdraw_pending() && !sources.owned().source, "quiesce drops every reservation");
    check(!sources.reserve(25, 17, 23, 0, 12).reserved.source && !sources.reserve(25, 17, 23, 4, 0).reserved.source,
        "reservation requires native job and poll generations");
    using rsf::skyrim::BindRefusal;
    using rsf::skyrim::bind_refusal;
    check(bind_refusal(true, false, false, false) == BindRefusal::none, "covered gameplay tick binds");
    check(bind_refusal(false, false, false, false) == BindRefusal::unreadable, "unreadable native gates refuse binding");
    check(bind_refusal(true, true, false, false) == BindRefusal::queued_move, "queued move consumes before the bind");
    check(bind_refusal(true, false, true, false) == BindRefusal::consumed_before_bind, "earlier hooked consumer refuses binding");
    check(bind_refusal(true, false, false, true) == BindRefusal::free_camera, "free camera bypasses the player envelope");
    rsf::skyrim::NativeScreenFacts screen{};
    using rsf::skyrim::classify_screen;
    check(classify_screen(screen) == RSF_SCREEN_UNKNOWN, "unreadable native screen refuses classification");
    screen.readable = screen.owned_main = screen.parent_cell = true;
    check(classify_screen(screen) == RSF_SCREEN_UNKNOWN, "no observed primary world is not gameplay");
    screen.primary_world = true;
    check(classify_screen(screen) == RSF_SCREEN_GAMEPLAY, "active unpaused primary world is gameplay");
    screen.freeze_time = true;
    check(classify_screen(screen) == RSF_SCREEN_UNKNOWN, "freeze alone is not proof of menu or loading");
    screen.pause_count = 1;
    check(classify_screen(screen) == RSF_SCREEN_MENU, "native paused primary world is menu");
    screen.loading_menu = true;
    check(classify_screen(screen) == RSF_SCREEN_LOADING, "native loading menu takes precedence over pause");
    screen.loading_menu = false; screen.pause_count = 0; screen.freeze_time = false;
    screen.main_menu = true;
    check(classify_screen(screen) == RSF_SCREEN_MENU, "native main menu overrides an existing parent cell");
    screen.main_menu = false; screen.frozen_background = true;
    check(classify_screen(screen) == RSF_SCREEN_MENU, "native cached menu background is menu");
    screen.frozen_background = false; screen.parent_cell = false;
    check(classify_screen(screen) == RSF_SCREEN_UNKNOWN, "missing parent cell refuses gameplay");
    screen.parent_cell = true; screen.requested_mode = 2;
    check(classify_screen(screen) == RSF_SCREEN_UNKNOWN, "independent manager mode refuses gameplay");
    screen.requested_mode = 0; screen.running_mode = 2;
    check(classify_screen(screen) == RSF_SCREEN_UNKNOWN, "mode shutdown before join refuses gameplay");
    screen.running_mode = 0; screen.owned_main = false; screen.loading_menu = true;
    check(classify_screen(screen) == RSF_SCREEN_UNKNOWN, "foreign main scope refuses classification");
    using rsf::skyrim::rendered_source_eligible;
    check(rendered_source_eligible(true, 1, RSF_SCREEN_GAMEPLAY), "one proven gameplay World admits rendered source");
    check(!rendered_source_eligible(true, 0, RSF_SCREEN_GAMEPLAY), "before World remains refused");
    check(!rendered_source_eligible(true, 2, RSF_SCREEN_GAMEPLAY), "multiple primary Worlds remain refused");
    check(!rendered_source_eligible(false, 1, RSF_SCREEN_GAMEPLAY), "inactive source scope remains refused");
    for (auto refused : {RSF_SCREEN_UNKNOWN, RSF_SCREEN_MENU, RSF_SCREEN_LOADING})
        check(!rendered_source_eligible(true, 1, refused), "unproven gameplay screen remains refused");
    rsf::skyrim::InputOwnership ownership;
    check(!ownership.snapshot().generation, "no producer does not establish ownership");
    auto poll = ownership.begin_poll();
    check(ownership.owns_poll(poll), "sole native poll has live callback provenance");
    check(!ownership.snapshot().generation, "in-flight poll is refused");
    ownership.end_poll(poll, true);
    const auto consumed = ownership.snapshot();
    check(consumed.generation == poll.generation, "completed poll has its own generation");
    check(ownership.unchanged(consumed), "consumer retains unchanged producer");
    poll = ownership.begin_poll();
    check(!ownership.unchanged(consumed), "next poll invalidates previous camera association");
    ownership.end_poll(poll, false);
    check(!ownership.snapshot().generation, "poll without mapped sink is refused");
    poll = ownership.begin_poll();
    auto overlapping = ownership.begin_poll();
    check(!ownership.owns_poll(poll) && !ownership.owns_poll(overlapping), "overlapping polls refuse callback provenance");
    ownership.end_poll(overlapping, true);
    ownership.end_poll(poll, true);
    check(!ownership.snapshot().generation, "overlapping polls refuse both generations");
    poll = ownership.begin_poll();
    overlapping = ownership.begin_poll();
    ownership.end_poll(poll, true);
    ownership.end_poll(overlapping, true);
    check(!ownership.snapshot().generation, "overlap refusal survives reversed completion order");
    poll = ownership.begin_poll();
    ownership.invalidate();
    ownership.end_poll(poll, true);
    check(!ownership.snapshot().generation, "unframed producer invalidates in-flight ownership");
    poll = ownership.begin_poll();
    ownership.end_poll(poll, true);
    const auto before_write = ownership.snapshot();
    ownership.begin_write();
    check(!ownership.snapshot().generation, "unframed sink write refuses consumption");
    ownership.end_write();
    check(!ownership.unchanged(before_write), "sink rewrite invalidates camera association");
    ownership.invalidate();
    check(!ownership.snapshot().generation, "quiesce invalidates old session input");
    using rsf::skyrim::cpu_input_kinds;
    Event event{}; Event* list = &event;
    check(cpu_input_kinds(&list) == RSF_GAME_INPUT_KEYBOARD, "keyboard button");
    event.device = 3; event.type = 2;
    check(cpu_input_kinds(&list) == RSF_GAME_INPUT_KEYBOARD, "virtual keyboard character");
    event.device = 1; event.type = 1;
    check(cpu_input_kinds(&list) == RSF_GAME_INPUT_MOUSE, "mouse movement");
    event.device = 2; event.type = 3;
    check(cpu_input_kinds(&list) == RSF_GAME_INPUT_CONTROLLER, "controller thumbstick");
    event.type = 4;
    check(cpu_input_kinds(&list) == 0, "device connection is not delivered control input");
    event.device = 99; event.type = 0;
    check(cpu_input_kinds(&list) == 0, "unknown device");
    event.device = 0; event.type = 1;
    check(cpu_input_kinds(&list) == 0, "mismatched device and movement type");
    Event mouse{0, 1, 0, nullptr}, controller{0, 2, 0, nullptr};
    event.type = 0; event.next = &mouse; mouse.next = &controller;
    check(cpu_input_kinds(&list) == 7, "mixed list aggregates delivered kinds");
    controller.next = &event;
    check(cpu_input_kinds(&list) == 0, "cyclic list refuses partial activity");
    event.next = reinterpret_cast<Event*>(1);
    check(cpu_input_kinds(&list) == 0, "unreadable node refuses partial activity");
    check(cpu_input_kinds(reinterpret_cast<const void*>(1)) == 0, "unreadable head pointer");
    check(cpu_input_kinds(nullptr) == 0, "absent head pointer");
    list = nullptr;
    check(cpu_input_kinds(&list) == 0, "empty list");
    std::array<Event, 129> long_list{};
    for (size_t i = 0; i + 1 < long_list.size(); ++i) long_list[i].next = &long_list[i + 1];
    list = long_list.data();
    check(cpu_input_kinds(&list) == 0, "excessive list refuses partial activity");
    long_list[127].next = nullptr;
    check(cpu_input_kinds(&list) == RSF_GAME_INPUT_KEYBOARD, "bounded list terminates successfully");
    return failures ? 1 : 0;
}

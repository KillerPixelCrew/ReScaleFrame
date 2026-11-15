// SPDX-License-Identifier: GPL-3.0-only
#include "../games/skyrim-se/src/cpu_events.h"
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

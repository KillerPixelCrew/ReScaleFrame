/* SPDX-License-Identifier: GPL-3.0-only */
/* Membership sets kept by address, and the eviction that makes that safe.
 *
 * No device. The module exists to hold answers that were decided elsewhere, so everything it does
 * can be driven from stand-in pointers, and the case that matters most is the one a real frame
 * produces without asking: an address handed out again for a different kind of object.
 */
#include <rescaleframe/ui_identify.h>

#include <cstdio>
#include <cstring>

namespace {

bool passed = true;

void check(bool condition, const char* what)
{
    if (!condition) {
        std::fprintf(stderr, "FAIL: %s\n", what);
        passed = false;
    }
}

void stage(const char* what) { std::fprintf(stderr, "[stage] %s\n", what); }

char slate_a;
char slate_b;
char canvas;
char widget;
char scene;

} // namespace

int main()
{
    stage("creation checks its ABI");
    check(rsf_ui_registry_create(RSF_UI_IDENTIFY_ABI_VERSION + 1u) == nullptr,
          "A registry compiled against another version of this header must be refused.");

    rsf_ui_registry* registry = rsf_ui_registry_create(RSF_UI_IDENTIFY_ABI_VERSION);
    check(registry != nullptr, "A registry must be created.");
    if (!registry) {
        return 1;
    }

    stage("membership");
    {
        check(rsf_ui_registry_add(registry, RSF_UI_SET_SLATE_LAYOUT, &slate_a) == RSF_UI_OK,
              "Recording a declaration must succeed.");
        check(rsf_ui_registry_add(registry, RSF_UI_SET_SLATE_LAYOUT, &slate_a) == RSF_UI_OK,
              "Recording the same object twice must be accepted and change nothing.");
        check(rsf_ui_registry_add(registry, RSF_UI_SET_CANVAS_LAYOUT, &canvas) == RSF_UI_OK,
              "A second set must be independent.");
        check(rsf_ui_registry_contains(registry, RSF_UI_SET_SLATE_LAYOUT, &slate_a) == 1,
              "A recorded object must be found.");
        check(rsf_ui_registry_contains(registry, RSF_UI_SET_CANVAS_LAYOUT, &slate_a) == 0,
              "And not in a set it was never added to.");
        check(rsf_ui_registry_contains(registry, RSF_UI_SET_SLATE_LAYOUT, &scene) == 0,
              "An object nobody recorded must be found nowhere, which is the answer for almost "
              "every draw in a frame.");

        uint32_t count = 0;
        void* const* view = rsf_ui_registry_view(registry, RSF_UI_SET_SLATE_LAYOUT, &count);
        check(view != nullptr && count == 1 && view[0] == &slate_a,
              "The view must be the set, contiguous, for handing to a game's own rule.");
    }

    stage("an address handed out again is forgotten first");
    {
        // The hazard, exactly as a frame produces it: something is released and the next creation
        // lands on its address. The registry must not still believe the old answer, because the new
        // object is live and would be diverted on the strength of what its predecessor was.
        check(rsf_ui_registry_contains(registry, RSF_UI_SET_SLATE_LAYOUT, &slate_a) == 1,
              "Precondition: the address is recorded.");
        rsf_ui_registry_forget(registry, &slate_a);
        check(rsf_ui_registry_contains(registry, RSF_UI_SET_SLATE_LAYOUT, &slate_a) == 0,
              "After forgetting, the address must mean nothing again.");

        // And the new object at that address is whatever the rule now says, including nothing.
        check(rsf_ui_registry_add(registry, RSF_UI_SET_WIDGET_TARGET, &slate_a) == RSF_UI_OK,
              "The same address may be recorded as something else entirely.");
        check(rsf_ui_registry_contains(registry, RSF_UI_SET_WIDGET_TARGET, &slate_a) == 1,
              "And is then that.");
        check(rsf_ui_registry_contains(registry, RSF_UI_SET_SLATE_LAYOUT, &slate_a) == 0,
              "Without ever being both.");

        rsf_ui_registry_forget(registry, &slate_a);
        rsf_ui_registry_forget(registry, &slate_a);
        check(rsf_ui_registry_contains(registry, RSF_UI_SET_WIDGET_TARGET, &slate_a) == 0,
              "Forgetting an address twice must be harmless: the caller forgets every creation "
              "without knowing whether the address was ever recorded.");
    }

    stage("the set stays intact when an entry in the middle goes");
    {
        rsf_ui_registry_forget(registry, &canvas);
        check(rsf_ui_registry_add(registry, RSF_UI_SET_SLATE_LAYOUT, &slate_a) == RSF_UI_OK &&
                  rsf_ui_registry_add(registry, RSF_UI_SET_SLATE_LAYOUT, &slate_b) == RSF_UI_OK &&
                  rsf_ui_registry_add(registry, RSF_UI_SET_SLATE_LAYOUT, &widget) == RSF_UI_OK,
              "Three declarations must be recorded.");
        rsf_ui_registry_forget(registry, &slate_b);
        check(rsf_ui_registry_contains(registry, RSF_UI_SET_SLATE_LAYOUT, &slate_a) == 1 &&
                  rsf_ui_registry_contains(registry, RSF_UI_SET_SLATE_LAYOUT, &widget) == 1,
              "Removing the middle entry must leave the others findable. The last entry fills the "
              "hole, so this is really a check that the fill happened.");
        check(rsf_ui_registry_contains(registry, RSF_UI_SET_SLATE_LAYOUT, &slate_b) == 0,
              "And the removed one must be gone.");
        uint32_t count = 0;
        rsf_ui_registry_view(registry, RSF_UI_SET_SLATE_LAYOUT, &count);
        check(count == 2, "The count must follow.");
    }

    stage("a full set refuses out loud");
    {
        static char targets[RSF_UI_MAX_WIDGET_TARGETS + 4];
        rsf_ui_result last = RSF_UI_OK;
        for (uint32_t index = 0; index < RSF_UI_MAX_WIDGET_TARGETS; ++index) {
            last = rsf_ui_registry_add(registry, RSF_UI_SET_WIDGET_TARGET, &targets[index]);
        }
        check(last == RSF_UI_OK, "Filling a set to capacity must succeed.");
        check(rsf_ui_registry_add(registry, RSF_UI_SET_WIDGET_TARGET,
                                  &targets[RSF_UI_MAX_WIDGET_TARGETS]) == RSF_UI_ERROR_FULL,
              "One past capacity must be refused rather than dropped, because a set that quietly "
              "stopped recording looks exactly like a rule that stopped matching.");

        rsf_ui_registry_counters counters{};
        counters.struct_size = sizeof(counters);
        check(rsf_ui_registry_get_counters(registry, &counters) == RSF_UI_OK,
              "Counters must be readable.");
        check(counters.refused_full[RSF_UI_SET_WIDGET_TARGET] == 1,
              "The refusal must be counted, so a run can say the rule is matching too much.");
        check(counters.forgotten_on_reuse >= 3,
              "And every eviction, so how often the reuse hazard occurs in a real frame stops "
              "being a guess.");
    }

    stage("arguments are checked");
    {
        check(rsf_ui_registry_add(nullptr, RSF_UI_SET_SLATE_LAYOUT, &scene) ==
                  RSF_UI_ERROR_INVALID_ARGUMENT,
              "A null registry.");
        check(rsf_ui_registry_add(registry, RSF_UI_SET_SLATE_LAYOUT, nullptr) ==
                  RSF_UI_ERROR_INVALID_ARGUMENT,
              "A null object: an unbound slot is not a member of anything.");
        check(rsf_ui_registry_add(registry, RSF_UI_SET_COUNT, &scene) ==
                  RSF_UI_ERROR_INVALID_ARGUMENT,
              "A set that does not exist.");
        check(rsf_ui_registry_contains(nullptr, RSF_UI_SET_SLATE_LAYOUT, &scene) == 0,
              "Containment against a null registry is false, not a crash: the draw path asks this "
              "before anything has been created.");
        uint32_t count = 99;
        check(rsf_ui_registry_view(registry, RSF_UI_SET_COUNT, &count) == nullptr && count == 0,
              "A view of a set that does not exist must be empty rather than unset.");
        rsf_ui_registry_counters short_counters{};
        short_counters.struct_size = 4;
        check(rsf_ui_registry_get_counters(registry, &short_counters) ==
                  RSF_UI_ERROR_INVALID_ARGUMENT,
              "A short counters structure.");
        rsf_ui_registry_forget(nullptr, &scene);
    }

    stage("shader hashes name a shader");
    {
        const char first[] = "DXBC first shader blob";
        const char second[] = "DXBC second shader blob";
        const uint32_t hash_first = rsf_ui_shader_hash(first, sizeof(first));
        check(hash_first == rsf_ui_shader_hash(first, sizeof(first)),
              "The same bytes must hash the same, which is the only property a name needs.");
        check(hash_first != rsf_ui_shader_hash(second, sizeof(second)),
              "Different bytes must hash differently here, or naming a shader in a settings file "
              "would name two.");
        check(rsf_ui_shader_hash(first, sizeof(first) - 1) != hash_first,
              "The length must be part of it, so a truncated blob cannot hash to the whole one's "
              "plausible wrong answer.");
        check(rsf_ui_shader_hash(nullptr, 8) == 0 && rsf_ui_shader_hash(first, 0) == 0,
              "Nothing to hash is zero rather than a value that could collide with a real shader.");

        // Castagnoli's check value: the CRC32C of "123456789" is 0xE3069283. Pinned so a rewrite
        // of the loop cannot quietly change what every settings file in existence refers to.
        const char check_vector[] = "123456789";
        check(rsf_ui_shader_hash(check_vector, 9) == 0xE3069283u,
              "The published CRC32C check value must come out, because these hashes are written "
              "into settings files by hand and must not move.");
    }

    rsf_ui_registry_destroy(registry);
    rsf_ui_registry_destroy(nullptr);

    std::fprintf(stderr, "%s\n", passed ? "ui_identify: all checks passed" : "ui_identify: FAILED");
    return passed ? 0 : 1;
}

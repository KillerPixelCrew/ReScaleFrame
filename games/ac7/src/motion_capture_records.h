// SPDX-License-Identifier: MIT
#pragma once

#include <rescaleframe/ac7_motion_capture.h>
#include <cstddef>
#include "motion_capture_session.h"

// Private C ABI. Records contain copied observations and numeric native identities.
struct EngineRecord {
    rsf_ac7_velocity_facts facts;
    uint64_t qpc, primitive, proxy, view;
    uint32_t interval, thread, component, index;
    float camera[3], origin[3], current[16], previous[16];
};

enum RootKind : uint32_t {
    postprocess_begin, postprocess_end,
    widget_prepare_before, widget_prepare_after,
    widget_queue_before, widget_queue_after,
    main_temporal_build_before, main_temporal_build_after,
    main_temporal_process_begin, main_temporal_process_end,
    graph_pass_begin, graph_pass_end,
    rhi_scope_begin, rhi_scope_restore,
};

struct RootRecord {
    uint64_t id, parent, qpc, owner, command_list, view;
    uint64_t velocity_reference, velocity_rhi, widget, target;
    uint64_t context, input_node, output_node, velocity_node, vtable;
    uint64_t family, pooled_target, queue_begin, queue_end, process_fn, desc_fn;
    uint64_t downsample, blur_x, blur_y, glow;
    uint64_t execution_scope, color_input, color_output, depth, motion;
    uint32_t kind, session, interval, thread, fields_valid, view_blob, node_blob;
    uint32_t native_frame, queue_uid, output_id, width, height, format;
    uint32_t commands, output_valid, native_role;
    char output_name[64];
    int32_t draw_size[2];
};

struct NativeRootScope { uint64_t id, view; };
enum CommandAdvance : uint32_t { command_stop, command_next, command_failed };
enum class MotionRootInstall : uint32_t {
    already_installed, rhi_refused, entry_refused, rolled_back, installed, partial_teardown,
};

struct MotionRootHookEffects {
    bool(*copy)(void* destination, const void* source, std::size_t size);
    bool(*install)(void** targets, uint32_t count);
    void(*remove)(void** targets, uint32_t count);
};

// Callbacks are synchronous and return normally even if their native effects fail.
using MotionHistoryCopy = bool(*)(float* destination, const float* source);

extern "C" {
MotionRootInstall rsf_ac7_capture_install_roots(unsigned char* base, const MotionRootHookEffects* effects);
bool rsf_ac7_capture_remove_roots(const MotionRootHookEffects* effects);
MotionRootInstall rsf_ac7_capture_install_engine(unsigned char* base, const MotionRootHookEffects* effects);
bool rsf_ac7_capture_remove_engine(const MotionRootHookEffects* effects);
bool rsf_ac7_capture_engine_installed();
const void* rsf_ac7_capture_current_command_address();

EngineRecord* rsf_ac7_capture_swap_engine(EngineRecord* event);
void rsf_ac7_capture_history(uint64_t primitive, bool found, const float* previous, MotionHistoryCopy copy);
void rsf_ac7_capture_has_velocity(uint64_t primitive, bool has_velocity);
bool rsf_ac7_capture_record_engine(MotionCaptureInterval capture, const EngineRecord* event, bool accepted);

void rsf_ac7_capture_init_root(RootRecord* root, RootKind kind, uint64_t owner);
NativeRootScope rsf_ac7_capture_root_scope();
NativeRootScope rsf_ac7_capture_enter_root(const RootRecord* root);
void rsf_ac7_capture_restore_root(const NativeRootScope* scope);
void rsf_ac7_capture_end_root(RootRecord* root, uint64_t qpc);
bool rsf_ac7_capture_record_root(const RootRecord* root);

bool rsf_ac7_capture_commands_changed(const RootRecord* root, uint64_t link, uint32_t uid);
CommandAdvance rsf_ac7_capture_associate_command(RootRecord* root, uint64_t command, uint64_t execute);
bool rsf_ac7_capture_execution_pass(const rsf_game_render_pass* pass);
void rsf_ac7_capture_execution_root(RootRecord* root);

bool rsf_ac7_capture_store_blob(MotionCaptureInterval capture, const unsigned char* bytes,
    std::size_t size, uint32_t* id);
// File effects run after the Rust record lock has been released.
void rsf_ac7_capture_publish_checkpoint(MotionWriteFile write, MotionCaptureInterval capture);
}

static_assert(sizeof(MotionRootHookEffects) == 24 && offsetof(MotionRootHookEffects, remove) == 16);
static_assert(sizeof(rsf_ac7_velocity_facts) == 60);
static_assert(sizeof(EngineRecord) == 264 && offsetof(EngineRecord, previous) == 200);
static_assert(sizeof(RootRecord) == 376 && offsetof(RootRecord, output_name) == 304);
static_assert(sizeof(NativeRootScope) == 16);

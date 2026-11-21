// SPDX-License-Identifier: MIT
#pragma once

#include <rescaleframe/d3d11_observer.h>
#include <cstddef>
#include "motion_capture_session.h"

struct MotionShaderFacts { uint32_t hash, constant_mask; };
struct MotionLayoutSnapshot {
    uint32_t count, copied;
    rsf_observer_layout_element elements[RSF_OBSERVER_MAX_LAYOUT_ELEMENTS];
};

using MotionDisassembleConstants = uint32_t(*)(const void* bytes, std::size_t size);
using MotionCopyUpload = bool(*)(void* user, const unsigned char* bytes, std::size_t size);

struct MotionConstantRecord { uint64_t buffer, upload_serial; };
enum MotionConstantSource : uint32_t {
    constant_stopped, constant_uploaded, constant_try_read, constant_missing, constant_failed,
};
enum MotionConstantRead : uint32_t { constant_read_stopped, constant_read_skipped, constant_read_reserved };

extern "C" {
// False means allocation failure. Invalid observations and budget refusals are handled.
bool rsf_ac7_capture_observe_shader(void* shader, uint32_t stage, const void* bytes, uint32_t size,
    MotionDisassembleConstants disassemble);
uint32_t rsf_ac7_capture_declared_constants(const unsigned char* text, std::size_t size);
MotionShaderFacts rsf_ac7_capture_shader_facts(void* shader, uint32_t stage);

bool rsf_ac7_capture_observe_layout(void* layout, const rsf_observer_layout_element* elements,
    uint32_t copied, uint32_t count);
bool rsf_ac7_capture_find_layout(void* layout, MotionLayoutSnapshot* out);

// A successful creation evicts a previous upload at the same address even without initial data.
bool rsf_ac7_capture_store_upload(MotionCaptureInterval capture, void* buffer, const unsigned char* bytes,
    uint32_t size, bool created);
// The copy callback only copies to caller-owned storage and returns normally while the cache is locked.
MotionConstantSource rsf_ac7_capture_constant_source(MotionCaptureInterval capture, void* buffer,
    bool lighting, MotionConstantRecord* record, void* user, MotionCopyUpload copy);
MotionConstantRead rsf_ac7_capture_reserve_constant_read(MotionCaptureInterval capture, uint32_t width, bool lighting);
void rsf_ac7_capture_constant_read_failed(MotionCaptureInterval capture);

bool rsf_ac7_capture_begin_lighting_pair(MotionCaptureInterval capture);
bool rsf_ac7_capture_take_lighting_after(MotionCaptureInterval capture);
bool rsf_ac7_capture_colour_snapshot_available(MotionCaptureInterval capture, bool lighting);
// Returns the capture-wide read ordinal or UINT32_MAX if the sample or budget expired.
uint32_t rsf_ac7_capture_reserve_colour_snapshot(MotionCaptureInterval capture, bool lighting);
void rsf_ac7_capture_colour_snapshot_failed(MotionCaptureInterval capture);
}

static_assert(sizeof(MotionShaderFacts) == 8 && offsetof(MotionShaderFacts, constant_mask) == 4);
static_assert(sizeof(rsf_observer_layout_element) == 20 && offsetof(rsf_observer_layout_element, per_instance) == 16);
static_assert(sizeof(MotionLayoutSnapshot) == 328 && offsetof(MotionLayoutSnapshot, elements) == 8);
static_assert(sizeof(MotionConstantRecord) == 16 && offsetof(MotionConstantRecord, upload_serial) == 8);

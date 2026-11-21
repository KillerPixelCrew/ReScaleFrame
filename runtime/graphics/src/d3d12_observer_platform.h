/* SPDX-License-Identifier: MIT */
#pragma once
#include <rescaleframe/d3d12_observer.h>
#include <stddef.h>

enum {
    RSF_OBSERVER12_FACTORY = 0,
    RSF_OBSERVER12_DEVICE = 1,
    RSF_OBSERVER12_QUEUE = 2,
    RSF_OBSERVER12_CHAIN = 3,
    RSF_OBSERVER12_CAPACITY = 8
};

struct rsf_observer12_hooks {
    int32_t (*initialize)(void);
    int32_t (*create)(void* target, void* detour, void** original);
    int32_t (*enable)(void* target);
    void (*remove)(void* target);
};
struct rsf_observer12_attachment {
    int32_t (*query_factory)(void*, void**);
    int32_t (*query_device)(void*, void**);
    int32_t (*query_queue)(void*, void**);
    uint32_t (*queue_type)(void*);
    int32_t (*bind_interface)(uint32_t kind, void*, size_t slot);
    void (*register_debug)(void*);
    int32_t (*describe_window)(void*, void**);
};

struct rsf_observer12_breadcrumb {
    const void* next;
    void* list;
    void* queue;
    uint32_t completed, count;
    const char* name;
};

typedef void (*rsf_observer12_log)(void* user, const char* text);
struct rsf_observer12_diagnostic_setup {
    int32_t (*query_settings)(void**);
    void (*enable_breadcrumbs)(void*);
    void (*enable_page_faults)(void*);
    int32_t (*query_debug)(void**);
    int32_t (*enable_debug)(void*);
};
/* Query results transfer one reference. DRED nodes are borrowed while the interface is held. */
struct rsf_observer12_removal {
    int32_t (*queue_device)(void*, void**);
    int32_t (*removed_reason)(void*);
    int32_t (*query_dred)(void*, void**);
    const void* (*breadcrumb_head)(void*);
    void (*breadcrumb)(const void*, rsf_observer12_breadcrumb*);
    uint32_t (*operation)(const void*, uint32_t);
    int32_t (*page_fault)(void*, uint64_t*);
};

extern "C" {
int32_t rsf_core_observer12_create(const rsf_d3d12_observer_setup*, rsf_d3d12_observer**,
    const rsf_observer12_hooks*);
int32_t rsf_core_observer12_bind(uint32_t kind, void* target, void* const* wrappers,
    const rsf_observer12_hooks*);
void* rsf_core_observer12_original(uint32_t kind, size_t index);
int32_t rsf_core_observer12_enter(uint32_t kind);
void rsf_core_observer12_leave(uint32_t kind);
int32_t rsf_core_observer12_attach(const rsf_d3d12_observer*, void*, uint32_t kind,
    const rsf_observer12_attachment*);
void rsf_core_observer12_factory_result(const rsf_observer12_attachment*, int32_t result, uint32_t outer,
    void* queue, void* window, void* const* chain);
void rsf_core_observer12_device_result(const rsf_observer12_attachment*, int32_t result, void* const* queue);
uint32_t rsf_core_observer12_present_begin(const rsf_observer12_attachment*, void* chain, uint32_t interval, uint32_t flags);
uint64_t rsf_core_observer12_submit_begin(void* queue, uint32_t count, void* const* lists);
void rsf_core_observer12_submit_end(void* queue, uint64_t id, uint32_t count, void* const* lists);
void rsf_core_observer12_present(void* chain, uint32_t interval, uint32_t flags, uint32_t begin);
int32_t rsf_core_observer12_diagnostics_enable(rsf_observer12_log, void* user, const rsf_observer12_diagnostic_setup*);
void rsf_core_observer12_debug_message(uint32_t severity, uint32_t id, const char* description);
void rsf_core_observer12_report_removal(void* queue, const rsf_observer12_removal*);
}

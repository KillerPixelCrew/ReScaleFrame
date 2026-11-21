/* SPDX-License-Identifier: MIT */
#pragma once
#include <stddef.h>
#include <stdint.h>

static_assert(sizeof(wchar_t) == 2);
struct rsf_observer11_module_file;

enum : uint32_t {
    RSF_OBSERVER11_MODULE_OK = 0,
    RSF_OBSERVER11_MODULE_OPEN_FAILED = 1,
    RSF_OBSERVER11_MODULE_READ_FAILED = 2,
    RSF_OBSERVER11_MODULE_INVALID_HEADERS = 3,
};

extern "C" {
uint32_t rsf_core_observer11_module_open(const wchar_t* path, uint32_t length,
    rsf_observer11_module_file** out);
uint64_t rsf_core_observer11_module_preferred_base(const rsf_observer11_module_file*);
// The returned bytes are borrowed until the file is closed.
const unsigned char* rsf_core_observer11_module_span(const rsf_observer11_module_file*,
    size_t rva, size_t count);
void rsf_core_observer11_module_close(rsf_observer11_module_file*);
}

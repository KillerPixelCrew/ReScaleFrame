/* SPDX-License-Identifier: MIT */
/* Shared between the module dump writer and its structured sidecar sections. Not installed. */

#ifndef RSF_MODULE_DUMP_INTERNAL_H
#define RSF_MODULE_DUMP_INTERNAL_H

#include <rescaleframe/module_dump.h>

#include <windows.h>

#include <stdio.h>
#include <stdint.h>

#include "module_dump_memory.h"
#include "module_dump_records.h"

void rsf_dump_collect_modules(rsf_module_entry* entries, size_t capacity, size_t* count);

double rsf_dump_now_ms(void);

rsf_dump_result rsf_dump_poll_code(double threshold, uint32_t poll_ms, uint32_t timeout_ms,
                                   uint32_t required_stable, double* last_entropy);

#endif /* RSF_MODULE_DUMP_INTERNAL_H */

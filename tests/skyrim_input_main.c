// SPDX-License-Identifier: MIT
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

extern bool rsf_skyrim_guarded_read(uintptr_t address, void* output, size_t size);
extern int rsf_skyrim_input_fixture(bool (*read)(uintptr_t, void*, size_t));

int main(void) { return rsf_skyrim_input_fixture(rsf_skyrim_guarded_read); }

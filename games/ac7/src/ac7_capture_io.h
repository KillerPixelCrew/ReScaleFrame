/* SPDX-License-Identifier: MIT */
#pragma once
#include <stdbool.h>
#include <stddef.h>

bool rsf_ac7_capture_write_file(const char* path, const unsigned char* data, size_t bytes);

/* SPDX-License-Identifier: MIT */
#pragma once
#include <stdint.h>

/* The typed native observer continues through the trampoline published by observers.rs. */
void* rsf_skyrim_observer_original(uint32_t);

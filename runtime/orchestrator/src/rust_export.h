/* SPDX-License-Identifier: MIT */
#pragma once
#ifdef _MSC_VER
#define RSF_EXPORT(name) __pragma(comment(linker, "/export:" #name))
#else
#define RSF_EXPORT(name) __asm__(".section .drectve\n.ascii \" -export:" #name "\"\n.text");
#endif

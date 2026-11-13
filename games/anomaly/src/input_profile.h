// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include "native_profile.h"
namespace rsf::anomaly {
struct InputProfile {
    NativeSite frame, keyboard, mouse;
    uint32_t frame_vtable_rva;
    const char* frame_code;
};
// CInput's primary pureFrame vtable slot 0 calls OnFrame; secondary bases are application activation callbacks.
// OnFrame tests pKeyboard +0x28, calls KeyUpdate, then tests pMouse +0x20 and calls MouseUpdate.
inline constexpr uint32_t input_keyboard_offset = 0x28, input_mouse_offset = 0x20;

// ---- Standard 1.5.3 DX11 ----
inline constexpr InputProfile input_standard{
    {"CInput::OnFrame", 0x67810, signature("48895c2408574883ec20833d0f81cd00")},
    {"CInput::KeyUpdate", 0x66db0, signature("4c8bdc534881ec40060000833d7a10cd")},
    {"CInput::MouseUpdate", 0x67020, signature("4c8bdc534881ec80060000488bd941c7")},
    0xa55a20,
    "48895c2408574883ec20833d0f81cd0000488bf9488b1dbd02cd007432ff833c0a000080bb100a0000007523e87f780300482b83080a0000488983f809000033c0488983200a0000488983280a0000488d0dd201cd00e84558fcff4869c0e803000033d248f735857fcd000305dbffcc0048837f28008987ec0b00007408488bcfe81af5ffff48837f20007408488bcfe87bf7ffff833d8480cd0000488b1d3502cd007413488d8bf8090000e8ef57fcff480183300a0000488b5c24304883c4205fc3"
};

// ---- AVX 1.5.3 DX11 ----
inline constexpr InputProfile input_avx{
    {"CInput::OnFrame", 0x66540, signature("48895c2408574883ec20833ddf73cc00")},
    {"CInput::KeyUpdate", 0x65ae0, signature("4c8bdc534881ec40060000833d4a03cc")},
    {"CInput::MouseUpdate", 0x65d50, signature("4c8bdc534881ec80060000488bd941c7")},
    0xa42a60,
    "48895c2408574883ec20833ddf73cc0000488bf9488b1d8df5cb007432ff833c0a000080bb100a0000007523e81f770300482b83080a0000488983f809000033c0488983200a0000488983280a0000488d0da2f4cb00e8356afcff4869c0e803000033d248f7355572cc000305abf2cb0048837f28008987ec0b00007408488bcfe81af5ffff48837f20007408488bcfe87bf7ffff833d5473cc0000488b1d05f5cb007413488d8bf8090000e8df69fcff480183300a0000488b5c24304883c4205fc3"
};
}

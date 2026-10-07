; SPDX-License-Identifier: GPL-3.0-only
; Windows x64 version.dll forwarding ABI. Each stub places its names[] index in eax; dispatch
; resolves the real function and tail-jumps to it with the original argument registers/stack.
; Keep index order synchronized with shim_loader.cpp, separately from .def export ordinals.
EXTERN rsf_version_resolve:PROC
.code
dispatch PROC
    ; 88h reserves 20h shadow space, saves integer/XMM arguments and aligns rsp for the resolver.
    ; Original caller stack arguments remain at their original locations after rsp is restored.
    sub rsp, 88h
    mov [rsp+20h], rcx
    mov [rsp+28h], rdx
    mov [rsp+30h], r8
    mov [rsp+38h], r9
    movdqu [rsp+40h], xmm0
    movdqu [rsp+50h], xmm1
    movdqu [rsp+60h], xmm2
    movdqu [rsp+70h], xmm3
    mov ecx, eax
    call rsf_version_resolve
    mov rcx, [rsp+20h]
    mov rdx, [rsp+28h]
    mov r8, [rsp+30h]
    mov r9, [rsp+38h]
    movdqu xmm0, [rsp+40h]
    movdqu xmm1, [rsp+50h]
    movdqu xmm2, [rsp+60h]
    movdqu xmm3, [rsp+70h]
    add rsp, 88h
    ; Tail dispatch preserves the caller's return address and target-specific return convention.
    jmp rax
dispatch ENDP
forward MACRO name, index
name PROC
    mov eax, index
    jmp dispatch
name ENDP
ENDM
forward GetFileVersionInfoA, 0
forward GetFileVersionInfoByHandle, 1
forward GetFileVersionInfoExA, 2
forward GetFileVersionInfoExW, 3
forward GetFileVersionInfoSizeA, 4
forward GetFileVersionInfoSizeExA, 5
forward GetFileVersionInfoSizeExW, 6
forward GetFileVersionInfoSizeW, 7
forward GetFileVersionInfoW, 8
forward VerFindFileA, 9
forward VerFindFileW, 10
forward VerInstallFileA, 11
forward VerInstallFileW, 12
forward VerLanguageNameA, 13
forward VerLanguageNameW, 14
forward VerQueryValueA, 15
forward VerQueryValueW, 16
END

; SPDX-License-Identifier: GPL-3.0-only
EXTERN rsf_xinput_resolve:PROC
.code
dispatch PROC
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
    call rsf_xinput_resolve
    mov rcx, [rsp+20h]
    mov rdx, [rsp+28h]
    mov r8, [rsp+30h]
    mov r9, [rsp+38h]
    movdqu xmm0, [rsp+40h]
    movdqu xmm1, [rsp+50h]
    movdqu xmm2, [rsp+60h]
    movdqu xmm3, [rsp+70h]
    add rsp, 88h
    jmp rax
dispatch ENDP
forward MACRO name, index
name PROC
    mov eax, index
    jmp dispatch
name ENDP
ENDM
forward rsf_xinput_dllmain, 0
forward XInputGetState, 1
forward XInputSetState, 2
forward XInputGetCapabilities, 3
forward XInputEnable, 4
forward XInputGetDSoundAudioDeviceGuids, 5
forward XInputGetBatteryInformation, 6
forward XInputGetKeystroke, 7
forward rsf_xinput_100, 8
forward rsf_xinput_101, 9
forward rsf_xinput_102, 10
forward rsf_xinput_103, 11
END

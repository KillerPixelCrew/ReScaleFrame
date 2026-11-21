/* SPDX-License-Identifier: MIT */
#pragma once
/* Internal boundary between the Unity package's D3D11 bridge (integrations/unity/native/
   bridge.cpp) and its Rust submission sequencing (bridge_interop.rs). Not a DLL ABI; scalars
   only, so it has no generated bindings.

   Rust owns the command ring's slot rotation (RSF_UNITY_FRAMES_SLOTS slots, bridge_state.h), each
   slot's completion value, the bridge's completion counter and the shared D3D11/D3D12 fence
   values. bridge.cpp keeps the devices, fences, waits, copies and command lists. Completion values
   are signalled on the bridge's own D3D12 completion fence; 0 stands for no fence. */
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif
/* The slot the next reconstruction records into. Writes the completion it must wait for before
   reusing the slot to *complete when non-null; 0 when the slot was never submitted. */
uint32_t rsf_unity_interop_slot(uint64_t* complete);
/* The last completion value handed out; 0 before any submission. */
uint64_t rsf_unity_interop_submitted(void);
/* Two consecutive shared fence values: the D3D11 upload signal, then the D3D12 hand-back.
   Zero when either output is null. */
uint32_t rsf_unity_interop_shared_pair(uint64_t* first, uint64_t* second);
/* The next completion value for a submission outside the ring (the Editor preview window). */
uint64_t rsf_unity_interop_completion(void);
/* The next completion value, recorded as slot's last submission. Zero for an invalid slot. */
uint64_t rsf_unity_interop_submit(uint32_t slot);
/* Moves the ring past slot once its completion was signalled. */
void rsf_unity_interop_advance(uint32_t slot);
/* Non-zero when nothing was submitted or completed reached the last completion value. */
uint32_t rsf_unity_interop_drained(uint64_t completed);
/* Forgets the ring, the completion counter and the shared fence values. */
void rsf_unity_interop_release(void);
#ifdef __cplusplus
}
#endif

// SPDX-License-Identifier: MIT
// Pixel-shader sampler LOD bias on the immediate context, formerly sampler_bias.cpp.
use std::ffi::c_void;
use std::ptr::null_mut;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_COMMONSHADER_SAMPLER_SLOT_COUNT, D3D11_DEVICE_CONTEXT_IMMEDIATE, D3D11_MIP_LOD_BIAS_MAX,
    D3D11_MIP_LOD_BIAS_MIN, D3D11_SAMPLER_DESC, ID3D11Device, ID3D11DeviceContext,
    ID3D11SamplerState,
};
use windows::core::Interface;
use windows_sys::Win32::System::Threading::GetCurrentThreadId;

const SLOT_COUNT: usize = D3D11_COMMONSHADER_SAMPLER_SLOT_COUNT as usize;
const CACHE_CAPACITY: usize = 32;

type Slots = [Option<ID3D11SamplerState>; SLOT_COUNT];

struct Entry {
    original: ID3D11SamplerState,
    replacement: ID3D11SamplerState,
    bias: f32,
}

pub(crate) struct SamplerBias {
    entries: [Option<Entry>; CACHE_CAPACITY],
    // Bound replacements keep a separate reference across cache eviction.
    owned: Slots,
    // The immediate context, its thread and device of the first applied call.
    owner: Option<(*mut c_void, u32, ID3D11Device)>,
    next: usize,
}

pub(crate) fn rsf_sampler_bias_create() -> *mut SamplerBias {
    crate::allocate_handle(SamplerBias {
        entries: [const { None }; CACHE_CAPACITY],
        owned: [const { None }; SLOT_COUNT],
        owner: None,
        next: 0,
    })
}

fn raw(sampler: &Option<ID3D11SamplerState>) -> *mut c_void {
    sampler.as_ref().map_or(null_mut(), Interface::as_raw)
}

// The facade provides exclusive access, live COM pointers and SLOT_COUNT borrowed baselines.
pub(crate) unsafe fn rsf_sampler_bias_apply(
    handle: *mut SamplerBias,
    context: *mut c_void,
    baselines: *const *mut c_void,
    slot_mask: u32,
    bias: f32,
    applied_mask: *mut u32,
) -> i32 {
    let Some(applied_mask) = (unsafe { applied_mask.as_mut() }) else {
        return 0;
    };
    *applied_mask = 0;
    let (Some(state), Some(baselines), Some(immediate)) = (unsafe {
        (
            handle.as_mut(),
            baselines.cast::<[*mut c_void; SLOT_COUNT]>().as_ref(),
            ID3D11DeviceContext::from_raw_borrowed(&context),
        )
    }) else {
        return 0;
    };
    if slot_mask >> SLOT_COUNT != 0
        || !(-16.0..=16.0).contains(&bias)
        || unsafe { immediate.GetType() } != D3D11_DEVICE_CONTEXT_IMMEDIATE
    {
        return 0;
    }
    let Ok(device) = (unsafe { immediate.GetDevice() }) else {
        return 0;
    };
    let thread = unsafe { GetCurrentThreadId() };
    if state
        .owner
        .as_ref()
        .is_some_and(|(owner, owner_thread, owner_device)| {
            *owner != context || *owner_thread != thread || *owner_device != device
        })
    {
        return 0;
    }
    let baselines: Slots = baselines
        .map(|sampler| unsafe { ID3D11SamplerState::from_raw_borrowed(&sampler) }.cloned());
    for sampler in baselines.iter().flatten() {
        if unsafe { sampler.GetDevice() }.ok().as_ref() != Some(&device) {
            return 0;
        }
    }
    let mut current: Slots = Default::default();
    unsafe { immediate.PSGetSamplers(0, Some(&mut current)) };
    let mut selected = current.clone();
    let mut applied = 0;
    for slot in 0..SLOT_COUNT {
        // Respect samplers installed by another renderer component.
        if raw(&current[slot]) != raw(&baselines[slot])
            && (state.owned[slot].is_none() || current[slot] != state.owned[slot])
        {
            continue;
        }
        selected[slot] = baselines[slot].clone();
        let Some(original) = baselines[slot]
            .as_ref()
            .filter(|_| slot_mask & (1 << slot) != 0 && bias != 0.0)
        else {
            continue;
        };
        let cached = state
            .entries
            .iter()
            .flatten()
            .find(|entry| entry.original == *original && entry.bias == bias);
        let replacement = if let Some(entry) = cached {
            entry.replacement.clone()
        } else {
            let mut desc = D3D11_SAMPLER_DESC::default();
            unsafe { original.GetDesc(&mut desc) };
            desc.MipLODBias += bias;
            if !(D3D11_MIP_LOD_BIAS_MIN..=D3D11_MIP_LOD_BIAS_MAX).contains(&desc.MipLODBias) {
                return 0;
            }
            let mut replacement = None;
            if unsafe { device.CreateSamplerState(&desc, Some(&mut replacement)) }.is_err() {
                return 0;
            }
            let Some(replacement) = replacement else {
                return 0;
            };
            state.entries[state.next] = Some(Entry {
                original: original.clone(),
                replacement: replacement.clone(),
                bias,
            });
            state.next = (state.next + 1) % CACHE_CAPACITY;
            replacement
        };
        selected[slot] = Some(replacement);
        applied |= 1 << slot;
    }
    for slot in 0..SLOT_COUNT {
        if raw(&selected[slot]) != raw(&current[slot]) {
            unsafe { immediate.PSSetSamplers(slot as u32, Some(&selected[slot..=slot])) };
        }
        state.owned[slot] = selected[slot]
            .clone()
            .filter(|_| applied & (1 << slot) != 0);
    }
    *applied_mask = applied;
    let ok = unsafe { device.GetDeviceRemovedReason() }.is_ok();
    state.owner = Some((context, thread, device));
    ok.into()
}

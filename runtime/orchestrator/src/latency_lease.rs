// SPDX-License-Identifier: MIT
use crate::render_abi::{RSF_PACING_NONE, RSF_PACING_REFLEX, RSF_PACING_XELL};
use std::os::windows::io::{FromRawHandle, OwnedHandle};
use std::sync::Mutex;
use windows_sys::Win32::{
    Foundation::{ERROR_ALREADY_EXISTS, ERROR_SUCCESS, GetLastError, SetLastError},
    System::Threading::{CreateEventW, GetCurrentProcessId},
};

struct Claim {
    client: u32,
    _event: OwnedHandle,
}

static CLAIM: Mutex<Option<Claim>> = Mutex::new(None);

pub(crate) fn pacing_owner(required: u32, reflex: bool, amd: bool, amd_owner: u32) -> u32 {
    if matches!(required, RSF_PACING_REFLEX | RSF_PACING_XELL) {
        required
    } else if reflex {
        RSF_PACING_REFLEX
    } else if amd {
        amd_owner
    } else {
        RSF_PACING_NONE
    }
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_native_latency_claim(client: u32) -> i32 {
    if !matches!(client, 1 | 2) {
        return 0;
    }
    let mut claim = CLAIM.lock().unwrap();
    if let Some(existing) = claim.as_ref() {
        return i32::from(existing.client == client);
    }
    // The named event arbitrates between separate DLL copies in this process.
    let process = unsafe { GetCurrentProcessId() };
    let name: Vec<u16> = format!("Local\\ReScaleFrame.LatencyOwner.{process}")
        .encode_utf16()
        .chain([0])
        .collect();
    let (event, error) = unsafe {
        SetLastError(ERROR_SUCCESS);
        let event = CreateEventW(std::ptr::null(), 1, 0, name.as_ptr());
        (event, GetLastError())
    };
    if event.is_null() {
        return 0;
    }
    let event = unsafe { OwnedHandle::from_raw_handle(event) };
    if error == ERROR_ALREADY_EXISTS {
        return 0;
    }
    *claim = Some(Claim {
        client,
        _event: event,
    });
    1
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_native_latency_has_claim(client: u32) -> i32 {
    i32::from(
        CLAIM
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|claim| claim.client == client),
    )
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_native_latency_release_claim(client: u32) {
    let mut claim = CLAIM.lock().unwrap();
    if claim.as_ref().is_some_and(|claim| claim.client == client) {
        *claim = None;
    }
}

#[cfg(test)]
mod tests {
    use super::pacing_owner;
    use crate::native_fg_cpu_abi::RSF_PACING_ANTILAG2;
    use crate::render_abi::{RSF_PACING_NONE, RSF_PACING_REFLEX, RSF_PACING_XELL};

    #[test]
    fn required_provider_wins_before_optional_pacing() {
        for (required, reflex, amd, expected) in [
            (RSF_PACING_XELL, true, true, RSF_PACING_XELL),
            (RSF_PACING_REFLEX, false, true, RSF_PACING_REFLEX),
            (RSF_PACING_NONE, true, true, RSF_PACING_REFLEX),
            (RSF_PACING_NONE, false, true, RSF_PACING_ANTILAG2),
            (RSF_PACING_NONE, false, false, RSF_PACING_NONE),
        ] {
            assert_eq!(
                pacing_owner(required, reflex, amd, RSF_PACING_ANTILAG2),
                expected
            );
        }
    }
}

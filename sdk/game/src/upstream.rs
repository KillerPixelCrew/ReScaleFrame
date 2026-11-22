// SPDX-License-Identifier: MIT
use crate::rsf_game_host_services;

/// Identifies the upstream host whose callbacks a process-lived D3D11 host retained.
pub fn same_upstream_identity(
    first: &rsf_game_host_services,
    second: &rsf_game_host_services,
) -> bool {
    first.session_id == second.session_id
        && first.user == second.user
        && first.log.map(|callback| callback as *const ())
            == second.log.map(|callback| callback as *const ())
        && first.cpu_event.map(|callback| callback as *const ())
            == second.cpu_event.map(|callback| callback as *const ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rsf_game_cpu_event;
    use core::ffi::{c_char, c_void};

    unsafe extern "C" fn log(_: *mut c_void, _: *const c_char) {}
    unsafe extern "C" fn event(_: *mut c_void, _: *const rsf_game_cpu_event) {}

    #[test]
    fn reuse_compares_all_four_identity_fields() {
        let original = rsf_game_host_services {
            session_id: 11,
            user: core::ptr::dangling_mut(),
            log: Some(log),
            cpu_event: Some(event),
            // The other fields do not participate in process host identity.
            ..unsafe { core::mem::zeroed() }
        };
        assert!(same_upstream_identity(&original, &original));
        for replacement in [
            rsf_game_host_services {
                session_id: 12,
                ..original
            },
            rsf_game_host_services {
                user: core::ptr::null_mut(),
                ..original
            },
            rsf_game_host_services {
                log: None,
                ..original
            },
            rsf_game_host_services {
                cpu_event: None,
                ..original
            },
        ] {
            assert!(!same_upstream_identity(&original, &replacement));
        }
    }
}

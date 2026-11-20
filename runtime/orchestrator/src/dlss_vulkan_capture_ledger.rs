// SPDX-License-Identifier: MIT
use crate::allocate_handle;
use crate::vulkan_fg_abi::rsf_dlss_capture_source as Source;
use std::collections::{BTreeMap, BTreeSet};

const AMBIGUOUS: u32 = 1;
const TOKEN_RANGE: u32 = 2;

#[derive(Default)]
struct Ledger {
    // None is a token for which two distinct capture identities were seen.
    sources: BTreeMap<u32, Option<Source>>,
    observed: Option<u32>,
    pending: BTreeSet<u32>,
    unsubmitted: bool,
    sequence: u64,
}

impl Ledger {
    fn remember(&mut self, source: Option<&Source>) -> u32 {
        self.observed = None;
        let Some(source) = source else {
            return 0;
        };
        let Ok(token) = u32::try_from(source.record.frame_id) else {
            return TOKEN_RANGE;
        };
        if token == 0 {
            return TOKEN_RANGE;
        }
        let slot = self.sources.entry(token).or_insert(Some(*source));
        let key = |source: &Source| {
            let record = &source.record;
            (
                record.session_id,
                record.frame_id,
                record.view_id,
                record.resource_generation,
            )
        };
        if !slot
            .as_ref()
            .is_some_and(|stored| key(stored) == key(source))
        {
            *slot = None;
            return AMBIGUOUS;
        }
        *slot = Some(*source);
        self.observed = Some(token);
        0
    }

    fn source(&self, token: u32) -> Option<&Source> {
        self.sources.get(&token)?.as_ref()
    }

    fn can_flush(&self) -> bool {
        !self.unsubmitted && self.pending.is_empty()
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_dlss_capture_create() -> *mut Ledger {
    allocate_handle(Ledger::default())
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dlss_capture_destroy(ledger: *mut Ledger) {
    if !ledger.is_null() {
        unsafe { drop(Box::from_raw(ledger)) };
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dlss_capture_remember(
    ledger: &mut Ledger,
    source: *const Source,
) -> u32 {
    ledger.remember(unsafe { source.as_ref() })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dlss_capture_source(
    ledger: &Ledger,
    token: u64,
    out: *mut Source,
) -> u32 {
    let Some(source) = u32::try_from(token)
        .ok()
        .and_then(|token| ledger.source(token))
    else {
        return 0;
    };
    unsafe { out.write(*source) };
    1
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dlss_capture_observed(ledger: &Ledger, out: *mut Source) -> u32 {
    let Some(source) = ledger.observed.and_then(|token| ledger.source(token)) else {
        return 0;
    };
    unsafe { out.write(*source) };
    1
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_dlss_capture_pending(ledger: &Ledger, family: u32) -> u32 {
    u32::from(ledger.pending.contains(&family))
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_dlss_capture_submitted(ledger: &mut Ledger, family: u32) {
    ledger.pending.insert(family);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_dlss_capture_completed(ledger: &mut Ledger, family: u32) {
    ledger.pending.remove(&family);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_dlss_capture_unsubmitted(ledger: &mut Ledger) {
    ledger.unsubmitted = true;
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_dlss_capture_can_record(ledger: &Ledger) -> u32 {
    u32::from(!ledger.unsubmitted)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_dlss_capture_can_flush(ledger: &Ledger) -> u32 {
    u32::from(ledger.can_flush())
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_dlss_capture_drained(ledger: &mut Ledger) {
    ledger.pending.clear();
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_dlss_capture_sequence(ledger: &mut Ledger) -> u64 {
    ledger.sequence += 1;
    ledger.sequence
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(frame: u64, view: u32) -> Source {
        let mut source: Source = unsafe { std::mem::zeroed() };
        source.record.frame_id = frame;
        source.record.session_id = 1;
        source.record.view_id = view;
        source.record.resource_generation = 1;
        source
    }

    #[test]
    fn exact_source_survives_updates_and_collisions_disable_the_token() {
        let mut ledger = Ledger::default();
        let first = source(7, 1);
        let mut refreshed = first;
        refreshed.generated_count = 3;
        assert_eq!(ledger.remember(Some(&first)), 0);
        assert_eq!(ledger.remember(Some(&refreshed)), 0);
        assert_eq!(ledger.source(7).unwrap().generated_count, 3);
        assert_eq!(ledger.remember(Some(&source(7, 2))), AMBIGUOUS);
        assert!(ledger.source(7).is_none() && ledger.observed.is_none());
        assert_eq!(ledger.remember(Some(&first)), AMBIGUOUS);
    }

    #[test]
    fn out_of_domain_sources_never_alias_a_retained_token() {
        let mut ledger = Ledger::default();
        assert_eq!(ledger.remember(Some(&source(7, 1))), 0);
        assert_eq!(
            ledger.remember(Some(&source(7 + (1u64 << 32), 2))),
            TOKEN_RANGE
        );
        assert_eq!(ledger.source(7).unwrap().record.view_id, 1);
        assert!(ledger.observed.is_none());
    }

    #[test]
    fn completion_proof_is_exact_to_a_family_and_excludes_unsubmitted_copies() {
        let mut ledger = Ledger::default();
        rsf_core_dlss_capture_submitted(&mut ledger, 1);
        rsf_core_dlss_capture_submitted(&mut ledger, 2);
        rsf_core_dlss_capture_completed(&mut ledger, 2);
        assert_eq!(rsf_core_dlss_capture_can_flush(&ledger), 0);
        rsf_core_dlss_capture_completed(&mut ledger, 1);
        assert_eq!(rsf_core_dlss_capture_can_flush(&ledger), 1);
        rsf_core_dlss_capture_submitted(&mut ledger, 1);
        rsf_core_dlss_capture_unsubmitted(&mut ledger);
        rsf_core_dlss_capture_drained(&mut ledger);
        assert_eq!(rsf_core_dlss_capture_can_record(&ledger), 0);
        assert_eq!(rsf_core_dlss_capture_can_flush(&ledger), 0);
    }
}

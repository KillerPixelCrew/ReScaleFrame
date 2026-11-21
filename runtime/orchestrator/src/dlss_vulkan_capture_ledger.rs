// SPDX-License-Identifier: MIT
use crate::vulkan_fg_abi::rsf_dlss_capture_source as Source;
use std::collections::{BTreeMap, BTreeSet};

pub(crate) const AMBIGUOUS: u32 = 1;
pub(crate) const TOKEN_RANGE: u32 = 2;

// Exact source identity and per-queue-family completion proof for the
// dedicated present capture. Its owner serializes every call.
#[derive(Default)]
pub(crate) struct Ledger {
    // None is a token for which two distinct capture identities were seen.
    sources: BTreeMap<u32, Option<Source>>,
    observed: Option<u32>,
    pending: BTreeSet<u32>,
    unsubmitted: bool,
    sequence: u64,
}

impl Ledger {
    // Null source clears the observation anchor. Codes report an ambiguous
    // identity or a source that cannot fit in the SDK token domain.
    pub(crate) fn remember(&mut self, source: Option<&Source>) -> u32 {
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
        if slot
            .as_ref()
            .is_none_or(|stored| key(stored) != key(source))
        {
            *slot = None;
            return AMBIGUOUS;
        }
        *slot = Some(*source);
        self.observed = Some(token);
        0
    }

    pub(crate) fn source(&self, token: u64) -> Option<&Source> {
        self.sources.get(&u32::try_from(token).ok()?)?.as_ref()
    }

    pub(crate) fn observed(&self) -> Option<&Source> {
        self.observed.and_then(|token| self.source(token.into()))
    }

    pub(crate) fn pending(&self, family: u32) -> bool {
        self.pending.contains(&family)
    }

    pub(crate) fn submitted(&mut self, family: u32) {
        self.pending.insert(family);
    }

    pub(crate) fn completed(&mut self, family: u32) {
        self.pending.remove(&family);
    }

    pub(crate) fn unsubmitted(&mut self) {
        self.unsubmitted = true;
    }

    pub(crate) fn can_record(&self) -> bool {
        !self.unsubmitted
    }

    pub(crate) fn can_flush(&self) -> bool {
        !self.unsubmitted && self.pending.is_empty()
    }

    // The caller has already drained the presenter and device. This proves
    // completion of submitted copies only.
    pub(crate) fn drained(&mut self) {
        self.pending.clear();
    }

    pub(crate) fn next_sequence(&mut self) -> u64 {
        self.sequence += 1;
        self.sequence
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn source(frame: u64, view: u32) -> Source {
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
        assert_eq!(ledger.observed().unwrap().generated_count, 3);
        assert_eq!(ledger.remember(Some(&source(7, 2))), AMBIGUOUS);
        assert!(ledger.source(7).is_none() && ledger.observed().is_none());
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
        assert!(ledger.source(7 + (1u64 << 32)).is_none());
        assert!(ledger.observed().is_none());
    }

    #[test]
    fn completion_proof_is_exact_to_a_family_and_excludes_unsubmitted_copies() {
        let mut ledger = Ledger::default();
        ledger.submitted(1);
        ledger.submitted(2);
        ledger.completed(2);
        assert!(!ledger.can_flush());
        ledger.completed(1);
        assert!(ledger.can_flush());
        ledger.submitted(1);
        ledger.unsubmitted();
        ledger.drained();
        assert!(!ledger.can_record());
        assert!(!ledger.can_flush());
    }
}

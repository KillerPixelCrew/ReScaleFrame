// SPDX-License-Identifier: MIT
use super::*;
use crate::shader_family::diagnostic_family_name;
use crate::shader_loader::outcome_name;
use std::fmt::Write;

const CLASSES: [&str; 9] = [
    "static_world",
    "rigid",
    "skinned",
    "single_bone",
    "soft_skinned",
    "tree",
    "detail",
    "no_visual",
    "tessellated",
];
const PAIRS_LIMIT: usize = 4096;

#[derive(Clone, Default)]
pub(super) struct Counters {
    pub frames: u64,
    pub windows: u64,
    pub clears: u64,
    pub target_created: u64,
    pub target_failed: u64,
    pub msaa_frames: u64,
    pub options_unreadable: u64,
    pub descriptor_unreadable: u64,
    pub target_samples_or_layers: u64,
    pub window_draws: u64,
    pub motion_draws: u64,
    valid_draws: u64,
    invalid_draws: u64,
    reasons: [u64; HistoryReason::InvalidMatrix as usize + 1],
    pub unpaired_draws: u64,
    pub replays: u64,
    pub replay_skipped_no_depth_write: u64,
    pub replay_failed: u64,
    pub rebinds: u64,
    pub already_bound: u64,
    pub unbinds: u64,
    pub close_unbinds: u64,
    pub conflicts: u64,
    pub binder_faults: u64,
    pub restores: u64,
    pub stale_restores: u64,
    pub snapshots: u64,
    pub camera_conflicts: u64,
    pub retirements: u64,
    pub upload_failures: u64,
    pub captures: u64,
    pub images: u64,
    pub capture_failed: u64,
    pub tree_replaced: u64,
    tree_stock: u64,
    pub tree_valid: u64,
    pub tree_invalid: u64,
    pub detail_replaced: u64,
    detail_stock: u64,
    pub detail_valid: u64,
    pub detail_invalid: u64,
    pub detail_items: u64,
    pub detail_items_matched: u64,
    pub detail_items_unmatched: u64,
    pub detail_conflicts: u64,
    pub foliage_poisoned: u64,
    pub foliage_poison_failed: u64,
    pub foliage_table_overflows: u64,
    class_valid: [u64; CLASSES.len()],
    class_invalid: [u64; CLASSES.len()],
    pub class_unpaired: [u64; CLASSES.len()],
    pub tess_swapped: u64,
    pub tess_no_variant: u64,
    pub tess_unlinked: u64,
    pub tess_geometry: u64,
    pub tess_pack_refused: u64,
    pub band_valid: [u64; DISTANCE_BANDS.len()],
    pub band_invalid: [u64; DISTANCE_BANDS.len()],
    pub band_unpaired: [u64; DISTANCE_BANDS.len()],
    pub band_no_previous: [u64; DISTANCE_BANDS.len()],
    pub band_stale_palette: [u64; DISTANCE_BANDS.len()],
    pub band_hq: [u64; DISTANCE_BANDS.len()],
    pub band_lq: [u64; DISTANCE_BANDS.len()],
    pub band_tess: [u64; DISTANCE_BANDS.len()],
    pairs: BTreeMap<(usize, usize, usize), u64>,
}

impl Counters {
    pub fn unpaired_pair(&mut self, stock: ShaderObjects, class: usize) {
        let key = (stock.vs as usize, stock.ps as usize, class);
        if let Some(count) = self.pairs.get_mut(&key) {
            *count += 1;
        } else if self.pairs.len() < PAIRS_LIMIT {
            self.pairs.insert(key, 1);
        }
    }

    pub fn reason(&mut self, reason: HistoryReason, class: usize) {
        let valid = reason == HistoryReason::Valid;
        self.reasons[reason as usize] += 1;
        if valid {
            self.valid_draws += 1;
            self.class_valid[class] += 1;
        } else {
            self.invalid_draws += 1;
            self.class_invalid[class] += 1;
        }
    }

    pub fn stock_foliage(&mut self, family: u8) {
        if family == FOLIAGE_TREE {
            self.tree_stock += 1;
        } else {
            self.detail_stock += 1;
        }
    }
}

impl Stage {
    pub(super) fn report(&self, log: Log) {
        let (now, pairs) = {
            let mut render = lock(&self.render);
            let pairs = std::mem::take(&mut render.counters.pairs);
            (render.counters.clone(), pairs)
        };
        let (shader, core) = {
            let loader = lock(&self.loader);
            (
                loader.stats,
                loader.pack.as_ref().map(|pack| pack.counters()),
            )
        };
        let old = std::mem::replace(&mut *lock(&self.previous_report), now.clone());
        let d = |field: fn(&Counters) -> u64| field(&now).wrapping_sub(field(&old));
        if let Some(core) = core {
            let [
                _,
                _,
                _,
                source,
                hash,
                anchor,
                compile,
                original,
                reflection,
                format,
                internal,
            ] = core.refusals;
            log.say(format!(
                "Runtime shaders intercepted={} runtime_built={} cache_hits={} refused={} source={source} hash={hash} \
                 anchor={anchor} compile={compile} original={original} reflection={reflection} format={format} internal={internal}",
                core.intercepted, core.built, core.cache_hits, core.refused
            ));
        }
        log.say(format!(
            "Anomaly motion window frames={} windows={} clears={} draws={} motion={} valid={} invalid={} unpaired={} \
             replays={} replay_no_depth_write={} replay_failed={} | rtv2 rebinds={} already={} unbinds={} close_unbinds={} \
             conflicts={} faults={} | restores={} stale={} snapshots={} camera_conflicts={} retired={} upload_failures={} msaa={}",
            d(|s| s.frames), d(|s| s.windows), d(|s| s.clears), d(|s| s.window_draws), d(|s| s.motion_draws),
            d(|s| s.valid_draws), d(|s| s.invalid_draws), d(|s| s.unpaired_draws), d(|s| s.replays),
            d(|s| s.replay_skipped_no_depth_write), d(|s| s.replay_failed), d(|s| s.rebinds), d(|s| s.already_bound),
            d(|s| s.unbinds), d(|s| s.close_unbinds), d(|s| s.conflicts), d(|s| s.binder_faults), d(|s| s.restores),
            d(|s| s.stale_restores), d(|s| s.snapshots), d(|s| s.camera_conflicts), d(|s| s.retirements),
            d(|s| s.upload_failures), d(|s| s.msaa_frames)
        ));
        let mut reasons = String::from("Anomaly motion invalid reasons");
        for i in 1..now.reasons.len() {
            let name = unsafe { CStr::from_ptr(rsf_anomaly_motion_history_reason_name(i as u8)) }
                .to_string_lossy();
            let _ = write!(
                reasons,
                " {name}={}",
                now.reasons[i].wrapping_sub(old.reasons[i])
            );
        }
        log.say(reasons);
        log.say(format!(
            "Anomaly motion target options_unreadable={} descriptor_unreadable={} samples_or_layers={} allocation_failed={}",
            d(|s| s.options_unreadable), d(|s| s.descriptor_unreadable),
            d(|s| s.target_samples_or_layers), d(|s| s.target_failed),
        ));
        log.say(format!(
            "Anomaly motion foliage tree replaced={} stock={} valid={} invalid={} | detail replaced={} stock={} \
             valid={} invalid={} | detail items={} matched={} unmatched={} conflicts={} | rows poisoned={} \
             poison_failed={} table_overflows={}",
            d(|s| s.tree_replaced), d(|s| s.tree_stock), d(|s| s.tree_valid), d(|s| s.tree_invalid),
            d(|s| s.detail_replaced), d(|s| s.detail_stock), d(|s| s.detail_valid), d(|s| s.detail_invalid),
            d(|s| s.detail_items), d(|s| s.detail_items_matched), d(|s| s.detail_items_unmatched), d(|s| s.detail_conflicts),
            d(|s| s.foliage_poisoned), d(|s| s.foliage_poison_failed), d(|s| s.foliage_table_overflows)
        ));
        log.say(format!(
            "Anomaly motion tessellation chains swapped={} no_variant={} unlinked={} geometry_shader={} pack_refused={}",
            d(|s| s.tess_swapped), d(|s| s.tess_no_variant), d(|s| s.tess_unlinked), d(|s| s.tess_geometry),
            d(|s| s.tess_pack_refused)
        ));
        let mut classes = String::from("Anomaly motion classes");
        for (label, current, old) in [
            ("valid", now.class_valid, old.class_valid),
            ("invalid", now.class_invalid, old.class_invalid),
            ("unpaired", now.class_unpaired, old.class_unpaired),
        ] {
            let _ = write!(classes, " | {label}");
            for ((class, current), old) in CLASSES.iter().zip(current).zip(old) {
                let _ = write!(classes, " {class}={}", current.wrapping_sub(old));
            }
        }
        log.say(classes);
        let mut bands = String::from("Anomaly motion character bands");
        for (band, name) in DISTANCE_BANDS.iter().enumerate() {
            let name = name.to_string_lossy();
            let _ = write!(
                bands,
                " | {name} valid={} invalid={} unpaired={} no_previous={} stale_palette={} hq={} lq={} tess={}",
                now.band_valid[band].wrapping_sub(old.band_valid[band]),
                now.band_invalid[band].wrapping_sub(old.band_invalid[band]),
                now.band_unpaired[band].wrapping_sub(old.band_unpaired[band]),
                now.band_no_previous[band].wrapping_sub(old.band_no_previous[band]),
                now.band_stale_palette[band].wrapping_sub(old.band_stale_palette[band]),
                now.band_hq[band].wrapping_sub(old.band_hq[band]),
                now.band_lq[band].wrapping_sub(old.band_lq[band]),
                now.band_tess[band].wrapping_sub(old.band_tess[band])
            );
        }
        log.say(bands);
        if !pairs.is_empty() {
            let mut pairs: Vec<_> = pairs.into_iter().collect();
            pairs.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
            let mut line = String::from(
                "Anomaly motion unpaired (draws without a linkable variant pair, invalid motion):",
            );
            {
                let registry = lock(&self.registry);
                for &((vs, ps, class), count) in pairs.iter().take(10) {
                    let _ = write!(
                        line,
                        " {}+{}[{}]={count}",
                        registry.name(vs),
                        registry.name(ps),
                        CLASSES[class]
                    );
                }
            }
            if pairs.len() > 10 {
                let _ = write!(line, " (+{} more pairs)", pairs.len() - 10);
            }
            log.say(line);
        }
        let mut shaders = String::from("Anomaly motion shaders (totals)");
        for (id, counts) in shader.outcomes.into_iter().enumerate() {
            let family = diagnostic_family_name(id as u8);
            for (id, count) in counts.into_iter().enumerate().filter(|(_, c)| *c != 0) {
                let outcome = outcome_name(id as u8);
                let _ = write!(shaders, " {family}.{outcome}={count}");
            }
        }
        let _ = write!(
            shaders,
            " frame_refusals={} | target created={} failed={} | captures={} images={} capture_failed={}",
            shader.frame_refusals,
            now.target_created,
            now.target_failed,
            now.captures,
            now.images,
            now.capture_failed
        );
        log.say(shaders);
    }
}

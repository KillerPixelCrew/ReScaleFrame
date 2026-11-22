// SPDX-License-Identifier: MIT
use crate::dxgi_format::{
    D24_UNORM_S8_UINT, D32_FLOAT, D32_FLOAT_S8X24_UINT, R24_UNORM_X8_TYPELESS, R24G8_TYPELESS,
    R32_TYPELESS, R32G8X24_TYPELESS,
};
use crate::sys::{
    RSF_ROLE_RULE_ANY_DEPTH_FORMAT, RSF_ROLE_RULE_FIXED_SIZE, RSF_ROLE_RULE_RENDER_RANGE,
    RSF_ROLE_RULE_RENDER_SIZED, rsf_frame_shape as Shape, rsf_role_rule as Rule,
    rsf_role_table as Table, rsf_role_verdict as Verdict, rsf_texture_facts as Facts,
};
use rsf_game_sys::ffi_util::has_capacity;

const UNKNOWN: u32 = 0;
const CONFIDENT: u32 = 0;
const CANDIDATE: u32 = 1;

fn is_depth_format(format: u32) -> bool {
    matches!(
        format,
        R32G8X24_TYPELESS
            | D32_FLOAT_S8X24_UINT
            | R32_TYPELESS
            | D32_FLOAT
            | R24G8_TYPELESS
            | D24_UNORM_S8_UINT
            | R24_UNORM_X8_TYPELESS
    )
}

fn at_render_resolution(rule: &Rule, facts: &Facts, shape: &Shape) -> bool {
    if shape.render_width != 0 && shape.render_height != 0 {
        return facts.width == shape.render_width && facts.height == shape.render_height;
    }
    let [width_divisor, height_divisor] = if rule.flags & RSF_ROLE_RULE_RENDER_RANGE != 0 {
        [rule.width, rule.height]
    } else {
        [2, 2]
    };
    if shape.output_width == 0 || shape.output_height == 0 {
        return false;
    }
    let min_width = (shape.output_width / width_divisor).max(1);
    let min_height = (shape.output_height / height_divisor).max(1);
    if !(min_width..=shape.output_width).contains(&facts.width)
        || !(min_height..=shape.output_height).contains(&facts.height)
    {
        return false;
    }
    // Without a recorded render size, require a matching aspect within the 1/64 tolerance.
    let width_cross = u128::from(facts.width) * u128::from(shape.output_height);
    let height_cross = u128::from(facts.height) * u128::from(shape.output_width);
    width_cross.abs_diff(height_cross) * 64 <= width_cross + height_cross
}

fn matches_rule(rule: &Rule, facts: &Facts, shape: &Shape) -> bool {
    if rule.flags & RSF_ROLE_RULE_RENDER_RANGE != 0
        && (rule.flags & RSF_ROLE_RULE_RENDER_SIZED == 0
            || rule.flags & RSF_ROLE_RULE_FIXED_SIZE != 0
            || rule.width == 0
            || rule.height == 0)
    {
        return false;
    }
    if facts.bind_flags & rule.bind_flags != rule.bind_flags {
        return false;
    }
    let format_matches = if rule.flags & RSF_ROLE_RULE_ANY_DEPTH_FORMAT != 0 {
        is_depth_format(facts.format)
    } else {
        facts.format == rule.format
    };
    if !format_matches {
        return false;
    }
    if rule.flags & RSF_ROLE_RULE_FIXED_SIZE != 0
        && (facts.width != rule.width || facts.height != rule.height)
    {
        return false;
    }
    rule.flags & RSF_ROLE_RULE_RENDER_SIZED == 0 || at_render_resolution(rule, facts, shape)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_classify_texture(
    table: *const Table,
    facts: *const Facts,
    shape: *const Shape,
    verdict: *mut Verdict,
) -> u32 {
    if !unsafe {
        has_capacity(table) && has_capacity(facts) && has_capacity(shape) && has_capacity(verdict)
    } {
        return 0;
    }
    let (table, facts, shape) = unsafe { (table.read(), facts.read(), shape.read()) };
    let count = table.count as usize;
    if count != 0 && (table.rules.is_null() || count > isize::MAX as usize / size_of::<Rule>()) {
        return 0;
    }
    let mut result = Verdict {
        struct_size: unsafe { verdict.cast::<u32>().read() },
        role: UNKNOWN,
        confidence: CONFIDENT,
    };
    if count != 0 {
        let rules = unsafe { std::slice::from_raw_parts(table.rules, count) };
        if let Some(rule) = matching_rule(rules, &facts, &shape) {
            result.role = rule.role;
            result.confidence = rule_confidence(rule, &facts, &shape);
        }
    }
    unsafe { verdict.write(result) };
    1
}

// The first rule a texture matches, for owners that hold a copied rule table.
pub fn matching_rule<'a>(rules: &'a [Rule], facts: &Facts, shape: &Shape) -> Option<&'a Rule> {
    // Zero samples or slices denote the default. Zero mips requests a full chain.
    if facts.sample_count > 1 || facts.mip_levels != 1 || facts.array_size > 1 {
        return None;
    }
    rules.iter().find(|rule| matches_rule(rule, facts, shape))
}

/// A match possible only because of the wider range needs corroboration in the bound set.
pub fn rule_confidence(rule: &Rule, facts: &Facts, shape: &Shape) -> u32 {
    if rule.flags & RSF_ROLE_RULE_RENDER_RANGE != 0
        && (shape.render_width == 0 || shape.render_height == 0)
        && (facts.width < shape.output_width / 2 || facts.height < shape.output_height / 2)
    {
        CANDIDATE
    } else {
        rule.confidence
    }
}

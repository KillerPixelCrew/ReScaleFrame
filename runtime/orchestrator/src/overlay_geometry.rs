// SPDX-License-Identifier: MIT
use crate::overlay_abi::rsf_overlay_texture_update as Update;
use crate::overlay_host_abi::rsf_overlay_draw_call as DrawCall;

pub(super) const TEXTURE_EDGE: u32 = 16384;

pub(super) fn valid_texture(update: &Update) -> bool {
    !update.pixels.is_null()
        && (1..=TEXTURE_EDGE).contains(&update.width)
        && (1..=TEXTURE_EDGE).contains(&update.height)
}

pub(super) fn patch_fits(update: &Update, width: u32, height: u32) -> bool {
    update
        .x
        .checked_add(update.width)
        .is_some_and(|right| right <= width)
        && update
            .y
            .checked_add(update.height)
            .is_some_and(|bottom| bottom <= height)
}

// The caller holds the panel's borrowed geometry through this draw.
pub(super) unsafe fn borrowed_draw_slice<'a, T>(data: *const T, count: u32) -> Option<&'a [T]> {
    if count == 0 {
        return Some(&[]);
    }
    let count = count as usize;
    let bytes = count.checked_mul(size_of::<T>())?;
    if data.is_null() || !data.is_aligned() || bytes > isize::MAX as usize {
        return None;
    }
    data.addr().checked_add(bytes)?;
    Some(unsafe { std::slice::from_raw_parts(data, count) })
}

pub(super) fn valid_indices(call: &DrawCall, indices: &[u32], vertex_count: u32) -> bool {
    let Some(available) = vertex_count.checked_sub(call.vertex_offset) else {
        return false;
    };
    let Some(end) = call.index_offset.checked_add(call.index_count) else {
        return false;
    };
    indices
        .get(call.index_offset as usize..end as usize)
        .is_some_and(|slice| slice.iter().all(|&index| index < available))
}

pub(super) fn draw_clip(call: &DrawCall, width: u32, height: u32) -> Result<Option<[i32; 4]>, ()> {
    let right = call.clip_x.saturating_add(call.clip_width).min(width);
    let bottom = call.clip_y.saturating_add(call.clip_height).min(height);
    if call.index_count == 0 || call.clip_x >= right || call.clip_y >= bottom {
        return Ok(None);
    }
    let [Ok(left), Ok(top), Ok(right), Ok(bottom)] =
        [call.clip_x, call.clip_y, right, bottom].map(i32::try_from)
    else {
        return Err(());
    };
    Ok(Some([left, top, right, bottom]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_valid_index_span_cannot_reach_past_the_vertex_base() {
        let draw = DrawCall {
            index_offset: 1,
            index_count: 2,
            vertex_offset: 2,
            clip_x: 0,
            clip_y: 0,
            clip_width: 1,
            clip_height: 1,
            texture_id: 1,
        };
        assert!(valid_indices(&draw, &[99, 0, 1], 4));
        assert!(!valid_indices(&draw, &[99, 0, 2], 4));
    }
}

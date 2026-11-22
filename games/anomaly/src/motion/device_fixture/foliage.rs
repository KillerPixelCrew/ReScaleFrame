// SPDX-License-Identifier: MIT
use super::super::foliage::{
    History, Rows, rsf_anomaly_foliage_detail, rsf_anomaly_foliage_poison,
};
use super::super::stage::abi::FoliageLayout;
use super::super::*;
use crate::shader_loader::Reflection;

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_motion_foliage_fixture_layout(
    stock: Option<&Reflection>,
    family: u8,
    check_fields: bool,
    still: bool,
) -> *const c_char {
    let Some(stock) = stock else {
        return c"stock reflection missing".as_ptr();
    };
    let layout = FoliageLayout::from_stock(family, stock);
    if !layout.usable {
        return c"the engine's staged constants cannot be located from the stock reflection"
            .as_ptr();
    }
    if check_fields {
        if family == FOLIAGE_TREE && (!layout.xform.present || layout.xform.size < 48) {
            return c"m_xform is three registers".as_ptr();
        }
        if !layout.consts.present {
            return c"consts missing".as_ptr();
        }
        if family == FOLIAGE_DETAIL {
            if layout.array.size != 3904 {
                return c"array[61 * 4]".as_ptr();
            }
            if layout.wave.present == still || layout.wind.present == still {
                return c"wave set".as_ptr();
            }
        }
    }
    ptr::null()
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_motion_detail_fixture_bytes(
    extent: u32,
    include_b: bool,
    before: &FoliageConstants,
    current: &FoliageConstants,
    previous_groups: &[[f32; 16]; 2],
    current_groups: &[[f32; 16]; 2],
    rows: &mut MaybeUninit<Rows>,
    bytes: &mut MaybeUninit<[u8; FOLIAGE_BYTES]>,
) -> *const c_char {
    const ROUTE: u64 = 0xfeed;
    let mut history = History::default();
    let mut previous = MaybeUninit::uninit();
    let mut remap = MaybeUninit::uninit();
    rsf_anomaly_foliage_poison(&mut previous);
    rsf_anomaly_foliage_poison(rows);
    let previous = unsafe { previous.assume_init_mut() };
    let rows = unsafe { rows.assume_init_mut() };
    let count = if include_b { 2 } else { 1 };
    previous[..count].copy_from_slice(&previous_groups[..count]);
    rows[..2].copy_from_slice(current_groups);

    let frame = |source| FrameIdentity {
        source,
        generation: 0,
        width: extent,
        height: extent,
    };
    history.begin(0, frame(1));
    rsf_anomaly_foliage_detail(Some(&mut history), 0, ROUTE, before, previous, &mut remap);
    history.begin(0, frame(2));
    rsf_anomaly_foliage_detail(Some(&mut history), 0, ROUTE, current, rows, &mut remap);
    let remap = unsafe { remap.assume_init() };
    if include_b {
        if !(remap.live == 2 && remap.matched == 2 && remap.route_found) {
            return c"both plants remapped".as_ptr();
        }
    } else if !(remap.live == 2 && remap.matched == 1 && remap.unmatched == 1) {
        return c"only plant A has history".as_ptr();
    }
    if !unsafe {
        rsf_anomaly_motion_pack_detail(
            &Mat4::IDENTITY,
            &remap.previous,
            remap.items.as_ptr(),
            remap.items.len(),
            remap.live,
            bytes,
        )
    } {
        let message = if include_b {
            c"pack detail history"
        } else {
            c"pack partial history"
        };
        return message.as_ptr();
    }
    ptr::null()
}

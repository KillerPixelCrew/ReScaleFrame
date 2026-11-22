// SPDX-License-Identifier: MIT
//! TrueSky composite_tile's cloud-depth resource owner and dispatch parameters.
#[cfg(windows)]
pub(crate) mod d3d11;

#[cfg(any(windows, test))]
struct Facts {
    viewport_width: f32,
    viewport_height: f32,
    far_width: u32,
    far_height: u32,
    grid_width: u32,
    grid_height: u32,
    hdr_bytes: u32,
}

#[repr(C)]
struct Params {
    size: [u32; 2],
    divisor: [u32; 2],
    grid: [u32; 2],
    minimum_km: f32,
    minimum_opacity: f32,
}

const _: () = assert!(size_of::<Params>() == 32);

#[cfg(any(windows, test))]
fn plan(facts: &Facts) -> Option<Params> {
    if !(16.0..=16_384.0).contains(&facts.viewport_width)
        || !(16.0..=16_384.0).contains(&facts.viewport_height)
        || facts.far_width != facts.grid_width
        || facts.far_height != facts.grid_height
        || facts.grid_width == 0
        || facts.grid_height == 0
        || facts.hdr_bytes < 640
    {
        return None;
    }
    let width = facts.viewport_width as u32;
    let height = facts.viewport_height as u32;
    // The rounded ratio is the measured TrueSky divisor, including one texel of padding.
    let divisor = |pixels: u32, grid: u32| ((pixels + grid / 2) / grid).max(1);
    Some(Params {
        size: [width, height],
        divisor: [
            divisor(width, facts.grid_width),
            divisor(height, facts.grid_height),
        ],
        grid: [facts.grid_width, facts.grid_height],
        minimum_km: 0.3,
        minimum_opacity: 0.5,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cloud_dispatch_refuses_nonfinite_extent_and_preserves_padded_grid() {
        let mut facts = Facts {
            viewport_width: 533.0,
            viewport_height: 300.0,
            far_width: 534,
            far_height: 300,
            grid_width: 534,
            grid_height: 300,
            hdr_bytes: 640,
        };
        let params = plan(&facts).expect("matching cloud bindings");
        assert_eq!(params.size, [533, 300]);
        assert_eq!(params.divisor, [1, 1]);
        assert_eq!(params.grid, [534, 300]);
        facts.viewport_width = f32::NAN;
        assert!(plan(&facts).is_none());
    }
}

// SPDX-License-Identifier: MIT
use crate::sync::lock;
use crate::sys::RSF_OVERLAY_FRAME_LIMIT_VRR;
use crate::sys::{RSF_FG_OFF, rsf_fg_status};
use std::ffi::c_void;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};
use windows::Win32::Graphics::Dxgi::IDXGISwapChain;
use windows::core::Interface;
use windows_sys::Win32::{
    Graphics::Gdi::{
        DEVMODEW, ENUM_CURRENT_SETTINGS, EnumDisplaySettingsW, GetMonitorInfoW,
        MONITOR_DEFAULTTOPRIMARY, MONITORINFOEXW, MonitorFromWindow,
    },
    System::SystemInformation::GetTickCount64,
};

struct Display {
    chain: usize,
    sampled: u64,
    refresh_mhz: u32,
}

static DISPLAY: Mutex<Display> = Mutex::new(Display {
    chain: 0,
    sampled: 0,
    refresh_mhz: 0,
});
static PACING_REFRESH: AtomicU32 = AtomicU32::new(0);

pub(super) extern "C" fn rsf_core_overlay_refresh_mhz(window: *mut c_void) -> u32 {
    if window.is_null() {
        return 0;
    }
    let mut info = MONITORINFOEXW::default();
    info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
    let mut mode = DEVMODEW {
        dmSize: size_of::<DEVMODEW>() as u16,
        ..Default::default()
    };
    let valid = unsafe {
        GetMonitorInfoW(
            MonitorFromWindow(window, MONITOR_DEFAULTTOPRIMARY),
            &mut info.monitorInfo,
        ) != 0
            && EnumDisplaySettingsW(info.szDevice.as_ptr(), ENUM_CURRENT_SETTINGS, &mut mode) != 0
            && mode.dmDisplayFrequency > 1
    };
    if valid {
        mode.dmDisplayFrequency.saturating_mul(1000)
    } else {
        0
    }
}

// Refresh is sampled on a presenting thread. Recheck periodically while the
// same chain remains active so moving it to another display updates the limit.
unsafe fn display_refresh(chain: *mut c_void) -> u32 {
    let id = chain as usize;
    let Some(chain) = (unsafe { IDXGISwapChain::from_raw_borrowed(&chain) }) else {
        return 0;
    };
    let now = unsafe { GetTickCount64() };
    let mut display = lock(&DISPLAY);
    if display.chain != id || now.wrapping_sub(display.sampled) >= 500 {
        display.chain = id;
        display.sampled = now;
        display.refresh_mhz = unsafe { chain.GetDesc() }
            .map_or(0, |desc| rsf_core_overlay_refresh_mhz(desc.OutputWindow.0));
    }
    display.refresh_mhz
}

pub(super) unsafe fn observe_pacing_display(setting: u32, chain: *mut c_void) {
    let refresh = if setting == RSF_OVERLAY_FRAME_LIMIT_VRR {
        unsafe { display_refresh(chain) }
    } else {
        0
    };
    PACING_REFRESH.store(refresh, Ordering::Relaxed);
}

pub(super) fn pacing_refresh_mhz() -> u32 {
    PACING_REFRESH.load(Ordering::Relaxed)
}

fn interval(setting: u32, refresh_mhz: u32, multiplier: u32) -> u32 {
    if setting != RSF_OVERLAY_FRAME_LIMIT_VRR {
        return setting;
    }
    if refresh_mhz == 0 {
        return 0;
    }
    // One output period plus 0.3 ms keeps the cap below the refresh ceiling.
    let refresh = u64::from(refresh_mhz);
    let output_us = (1_000_000_000 + refresh / 2) / refresh + 300;
    (output_us * u64::from(multiplier)).min(u64::from(u32::MAX)) as u32
}

fn effective_interval(setting: u32, refresh_mhz: u32, fg: &rsf_fg_status) -> u32 {
    let multiplier = if fg.active != 0 && fg.effective_mode != RSF_FG_OFF {
        fg.effective_generated_frames.saturating_add(1)
    } else {
        1
    };
    interval(setting, refresh_mhz, multiplier)
}

pub(super) fn application_interval(setting: u32, fg: &rsf_fg_status) -> u32 {
    effective_interval(setting, pacing_refresh_mhz(), fg)
}

pub(super) fn reflex_interval(setting: u32, presents: u32) -> u32 {
    // Reflex takes a per-output interval. Both fixed caps and the VRR policy
    // keep their existing interpretation at this boundary.
    interval(setting, pacing_refresh_mhz(), 1)
        / if setting == RSF_OVERLAY_FRAME_LIMIT_VRR {
            1
        } else {
            presents.max(1)
        }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vrr_uses_confirmed_outputs_and_recalculates_for_a_new_display() {
        use crate::sys::RSF_FG_FIXED;

        const VRR: u32 = RSF_OVERLAY_FRAME_LIMIT_VRR;
        let mut fg = rsf_fg_status {
            effective_mode: RSF_FG_FIXED,
            effective_generated_frames: 3,
            ..unsafe { std::mem::zeroed() }
        };
        assert_eq!(effective_interval(VRR, 120_000, &fg), 8_633);
        fg.active = 1;
        assert_eq!(effective_interval(VRR, 120_000, &fg), 34_532);
        assert_eq!(effective_interval(VRR, 0, &fg), 0);
        assert_eq!(effective_interval(8_000, 120_000, &fg), 8_000);
        fg.effective_mode = RSF_FG_OFF;
        assert_eq!(effective_interval(VRR, 60_000, &fg), 16_967);
    }
}

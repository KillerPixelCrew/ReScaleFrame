// SPDX-License-Identifier: MIT
use rsf_game_sys::{RSF_GAME_INPUT_KEYBOARD, RSF_GAME_INPUT_MOUSE};

/// Classifies the Win32 message using the raw header type only for WM_INPUT.
pub fn classify_window_message(message: u32, raw_type: impl FnOnce() -> Option<u32>) -> u32 {
    match message {
        0x0100..=0x0109 => RSF_GAME_INPUT_KEYBOARD, // WM_KEYFIRST..=WM_KEYLAST
        0x0200..=0x020e => RSF_GAME_INPUT_MOUSE,    // WM_MOUSEFIRST..=WM_MOUSELAST
        0x00ff => match raw_type() {
            Some(0) => RSF_GAME_INPUT_MOUSE,    // RIM_TYPEMOUSE
            Some(1) => RSF_GAME_INPUT_KEYBOARD, // RIM_TYPEKEYBOARD
            _ => 0,
        },
        _ => 0,
    }
}

/// Classifies a message while its WM_INPUT handle is available.
#[cfg(windows)]
#[unsafe(no_mangle)]
pub extern "C" fn rsf_win32_input_kind(message: u32, lparam: isize) -> u32 {
    use windows_sys::Win32::UI::Input::{GetRawInputData, RAWINPUTHEADER, RID_HEADER};

    classify_window_message(message, || {
        let mut header = RAWINPUTHEADER::default();
        let header_size = size_of::<RAWINPUTHEADER>() as u32;
        let mut size = header_size;
        // Win32 validates the opaque handle and writes at most the supplied header size.
        let read = unsafe {
            GetRawInputData(
                lparam as _,
                RID_HEADER,
                (&raw mut header).cast(),
                &mut size,
                header_size,
            )
        };
        (read == header_size).then_some(header.dwType)
    })
}

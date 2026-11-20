// SPDX-License-Identifier: MIT
use super::{FRAMES, Frame, INVALID, LegacyPresent, NOT_READY};

const TOKEN_SLOTS: usize = 6; // The pinned SDK's retained frame-token ring.
const PCL_PING: u32 = 8;
const CONTROLLER_SAMPLE: u32 = 13;
const CONTROLLER: u32 = 4;
const INPUT_KINDS: u32 = 7;
const FEATURE_FAILED: i32 = -9;

#[derive(Clone, Copy)]
struct Token {
    frame: Frame,
    handle: usize,
    marked: u32,
    pacing_attempted: bool,
}

pub(super) struct StreamlineFrames {
    session: u64,
    tokens: [Option<Token>; TOKEN_SLOTS],
    pacing: bool,
}

impl StreamlineFrames {
    pub(super) const fn new() -> Self {
        Self {
            session: 0,
            tokens: [None; TOKEN_SLOTS],
            pacing: false,
        }
    }

    fn slot(&mut self, id: u64) -> &mut Option<Token> {
        &mut self.tokens[(id % TOKEN_SLOTS as u64) as usize]
    }

    pub(super) fn cancel(&mut self, frame: Frame) {
        let slot = self.slot(frame.id);
        if slot.is_some_and(|token| token.frame == frame) {
            *slot = None;
        }
    }
}

#[repr(C)]
pub(crate) struct SdkTokens {
    begin: unsafe extern "C" fn(u32) -> usize,
    current: unsafe extern "C" fn() -> usize,
    matches: unsafe extern "C" fn(usize, u32) -> i32,
    mark: unsafe extern "C" fn(usize, u32) -> i32,
}

// The caller holds the existing SDK mutex for every token operation. Rust holds
// the frame lock only to read or commit bookkeeping, never for a vendor call.
fn validated(sdk: &SdkTokens, session: u64, id: u64) -> Option<Token> {
    let index = u32::try_from(id).ok().filter(|index| *index != 0)?;
    let token = (*FRAMES.lock().unwrap().streamline.slot(id))?;
    (token.frame.id == id
        && (session == 0 || token.frame.session == session)
        && unsafe { (sdk.matches)(token.handle, index) } != 0)
        .then_some(token)
}

fn emit(sdk: &SdkTokens, token: &mut Token, kind: u32) -> i32 {
    let Some(mask) = 1_u32.checked_shl(kind) else {
        return INVALID;
    };
    if token.marked & mask != 0 {
        return 0;
    }
    if unsafe { (sdk.mark)(token.handle, kind) } != 0 {
        return FEATURE_FAILED;
    }
    token.marked |= mask;
    *FRAMES.lock().unwrap().streamline.slot(token.frame.id) = Some(*token);
    0
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_core_latencyvk_token_begin(session: u64, id: u64, sdk: &SdkTokens) {
    let frame = Frame { session, id };
    let index = {
        let mut frames = FRAMES.lock().unwrap();
        // The Reflex sleep leaf consumes the SDK's current token. Keep that
        // token stable across the unlocked options and sleep calls.
        if frames.streamline.pacing {
            return;
        }
        if frames.streamline.session != session {
            frames.streamline = StreamlineFrames::new();
            frames.streamline.session = session;
            frames.legacy = LegacyPresent::default();
        }
        let slot = frames.streamline.slot(id);
        if slot.is_some_and(|token| token.frame == frame) {
            return;
        }
        let Ok(index) = u32::try_from(id) else {
            return;
        };
        *slot = None;
        index
    };
    let handle = unsafe { (sdk.begin)(index) };
    if handle != 0 {
        *FRAMES.lock().unwrap().streamline.slot(id) = Some(Token {
            frame,
            handle,
            marked: 0,
            pacing_attempted: false,
        });
    }
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_core_latencyvk_token_lookup(
    session: u64,
    id: u64,
    sdk: &SdkTokens,
) -> usize {
    validated(sdk, session, id).map_or(0, |token| token.handle)
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_core_latencyvk_token_pace_begin(
    session: u64,
    id: u64,
    sdk: &SdkTokens,
) -> i32 {
    let Some(mut token) = validated(sdk, session, id) else {
        return 0;
    };
    if token.pacing_attempted || unsafe { (sdk.current)() } != token.handle {
        return 0;
    }
    let mut frames = FRAMES.lock().unwrap();
    if frames.streamline.pacing {
        return 0;
    }
    token.pacing_attempted = true;
    *frames.streamline.slot(id) = Some(token);
    frames.streamline.pacing = true;
    1
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_core_latencyvk_token_pace_end() {
    FRAMES.lock().unwrap().streamline.pacing = false;
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_core_latencyvk_token_pacing() -> i32 {
    i32::from(FRAMES.lock().unwrap().streamline.pacing)
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_core_latencyvk_token_mark(
    session: u64,
    id: u64,
    kind: u32,
    sdk: &SdkTokens,
) {
    if let Some(mut token) = validated(sdk, session, id) {
        let _ = emit(sdk, &mut token, kind);
    }
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_core_latencyvk_token_input(
    session: u64,
    id: u64,
    kind: u32,
    message: u32,
    pcl_message: u32,
    sdk: &SdkTokens,
) -> i32 {
    let Some(mut token) = validated(sdk, session, id) else {
        return NOT_READY;
    };
    if kind & !INPUT_KINDS != 0 || (message != 0 && message != pcl_message) {
        return INVALID;
    }
    // Keyboard and mouse timing uses real PCL pings, with a separate marker
    // for controller input. An unrelated window message contributes neither.
    if kind & CONTROLLER != 0 {
        let result = emit(sdk, &mut token, CONTROLLER_SAMPLE);
        if result != 0 {
            return result;
        }
    }
    if message != 0 {
        emit(sdk, &mut token, PCL_PING)
    } else if kind != 0 {
        0
    } else {
        INVALID
    }
}

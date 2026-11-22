// SPDX-License-Identifier: MIT
use super::{FRAMES, Frame, INVALID, LegacyPresent, NOT_READY};
use crate::input_receipts::streamline_input;
use crate::sys::rsf_vk_token_calls as SdkTokens;
use crate::vulkan_latency::rsf_core_latencyvk_sdk_result as sdk_result;

const TOKEN_SLOTS: usize = 6; // The pinned SDK's retained frame-token ring.
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

// The host serializes ledger operations. It drops its guard for Reflex sleep
// so older Present markers can still look up and use their retained tokens.
// The frame lock only covers bookkeeping.
fn validated(sdk: &SdkTokens, session: u64, id: u64) -> Option<Token> {
    let index = u32::try_from(id).ok().filter(|index| *index != 0)?;
    let token = (*FRAMES.lock().unwrap().streamline.slot(id))?;
    (token.frame.id == id
        && (session == 0 || token.frame.session == session)
        && unsafe { (sdk.matches.unwrap())(token.handle, index) } != 0)
        .then_some(token)
}

fn emit(sdk: &SdkTokens, token: &mut Token, kind: u32) -> i32 {
    let Some(mask) = 1_u32.checked_shl(kind) else {
        return INVALID;
    };
    if token.marked & mask != 0 {
        return 0;
    }
    let result = unsafe { (sdk.mark.unwrap())(token.handle, kind) };
    sdk_result(result, 1);
    if result != 0 {
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
    let mut handle = 0;
    let result = unsafe { (sdk.begin.unwrap())(index, &mut handle) };
    sdk_result(result, 0);
    if result == 0 && handle != 0 {
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
    if token.pacing_attempted || unsafe { (sdk.current.unwrap())() } != token.handle {
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
    let Some(input) = streamline_input(kind, message, pcl_message) else {
        return INVALID;
    };
    if input.foreign_message || input.empty {
        return INVALID;
    }
    if let Some(marker) = input.controller {
        let result = emit(sdk, &mut token, marker);
        if result != 0 {
            return result;
        }
    }
    input.ping.map_or(0, |marker| emit(sdk, &mut token, marker))
}

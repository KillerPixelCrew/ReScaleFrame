// SPDX-License-Identifier: MIT
use super::*;
use crate::capture_platform_abi::{
    rsf_ac7_capture_initialize_minhook, rsf_ac7_capture_install_engine_batch,
    rsf_ac7_capture_install_root_batch, rsf_ac7_capture_remove_hook_batch, rsf_ac7_copy_memory,
};
use crate::motion_capture_session::{rsf_ac7_capture_enabled, say};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::core::PCWSTR;

unsafe extern "C" {
    fn rsf_ac7_capture_end_session();
}

const ROOT_EFFECTS: RootHookEffects = RootHookEffects {
    copy: rsf_ac7_copy_memory,
    install: rsf_ac7_capture_install_root_batch,
    remove: rsf_ac7_capture_remove_hook_batch,
};
const ENGINE_EFFECTS: RootHookEffects = RootHookEffects {
    copy: rsf_ac7_copy_memory,
    install: rsf_ac7_capture_install_engine_batch,
    remove: rsf_ac7_capture_remove_hook_batch,
};

#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_motion_capture_install() -> i32 {
    if rsf_ac7_capture_engine_installed() {
        return 1;
    }
    if !rsf_ac7_capture_enabled() {
        return 0;
    }
    let base =
        unsafe { GetModuleHandleW(PCWSTR::null()) }.map_or(null_mut(), |module| module.0.cast());
    if !unsafe { rsf_ac7_capture_initialize_minhook() } {
        say(c"motion capture: MinHook unavailable");
        return 0;
    }
    let root = unsafe { rsf_ac7_capture_install_roots(base, &ROOT_EFFECTS) };
    let message = match root {
        RootInstall::AlreadyInstalled => None,
        RootInstall::RhiRefused => Some(c"renderer root capture refused: RHI current-command write differs"),
        RootInstall::EntryRefused => Some(c"renderer root capture refused: expected AC7 entry bytes differ"),
        RootInstall::RolledBack => Some(c"renderer root capture failed and was rolled back"),
        RootInstall::PartialTeardown => Some(c"renderer root capture has hooks pending removal"),
        RootInstall::Installed => Some(c"renderer root capture installed: graph/RHI associations, main temporal graph and widget producers; engine arguments unchanged"),
    };
    if let Some(message) = message {
        say(message);
    }
    let (installed, message) = match rsf_ac7_capture_install_engine(base, &ENGINE_EFFECTS) {
        RootInstall::AlreadyInstalled => return 1,
        RootInstall::RhiRefused | RootInstall::EntryRefused => (
            0,
            c"motion capture engine hooks refused: expected AC7 bytes differ",
        ),
        RootInstall::RolledBack => (
            0,
            c"motion capture engine hooks failed and were rolled back",
        ),
        RootInstall::PartialTeardown => (0, c"motion capture engine hooks pending removal"),
        RootInstall::Installed => (
            1,
            c"motion capture engine hooks installed; original decisions preserved",
        ),
    };
    say(message);
    installed
}

#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_motion_capture_shutdown() {
    unsafe { rsf_ac7_capture_end_session() };
    if !rsf_ac7_capture_remove_engine(&ENGINE_EFFECTS) {
        say(c"motion capture engine hooks pending removal");
    }
    if !rsf_ac7_capture_remove_roots(&ROOT_EFFECTS) {
        say(c"renderer root capture shutdown left hooks pending removal");
    }
}

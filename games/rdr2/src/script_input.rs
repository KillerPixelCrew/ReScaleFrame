// SPDX-License-Identifier: MIT
//! Optional ScriptHook input guard and engine settings poll on the registered game fiber.
use crate::OK;
use crate::engine_settings::quality_tick;
use crate::runtime_worker::{append_path, log};
use crate::sys::{RSF_RENDER_FRONTEND_ABI_VERSION, rsf_render_settings};
use rsf_game_sys::win32::typed_export;
use std::sync::OnceLock;
use windows_sys::Win32::Foundation::HMODULE;
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, LoadLibraryW};
use windows_sys::Win32::System::SystemInformation::GetTickCount64;
use windows_sys::Win32::System::Threading::GetCurrentProcessId;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_F7};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
use windows_sys::core::w;

const CONTROL_PRESSED: u64 = 0xe258_7f8c_bbd8_7b1d;
const DISABLE_CONTROL_ACTIONS: u64 = 0x5f4b_6931_816e_599b;
const LEFT_STICK: u64 = 0x43cd_a5b0;
const RIGHT_STICK: u64 = 0x7da4_8d2a;

type Register = unsafe extern "C" fn(HMODULE, extern "C" fn());
type GetSettings = unsafe extern "C" fn(*mut rsf_render_settings) -> i32;

#[derive(Clone, Copy)]
pub(crate) struct ScriptHook {
    register: Register,
    native_init: unsafe extern "C" fn(u64),
    native_push: unsafe extern "C" fn(u64),
    native_call: unsafe extern "C" fn() -> *const u64,
    wait: unsafe extern "C" fn(u32),
}

struct Input {
    script_hook: ScriptHook,
    visible: unsafe extern "C" fn() -> u32,
    toggle: unsafe extern "C" fn(),
    settings_get: Option<GetSettings>,
}

static INPUT: OnceLock<Input> = OnceLock::new();

impl ScriptHook {
    pub(crate) fn resolve(library: HMODULE) -> Option<Self> {
        // The names and types match the native ScriptHook exports used by both adapters.
        unsafe {
            Some(Self {
                register: typed_export(library, c"?scriptRegister@@YAXPEAUHINSTANCE__@@P6AXXZ@Z")?,
                native_init: typed_export(library, c"?nativeInit@@YAX_K@Z")?,
                native_push: typed_export(library, c"?nativePush64@@YAX_K@Z")?,
                native_call: typed_export(library, c"?nativeCall@@YAPEA_KXZ")?,
                wait: typed_export(library, c"?scriptWait@@YAXK@Z")?,
            })
        }
    }

    /// Publishes a process-lived callback from the ASI.
    pub(crate) unsafe fn register(&self, instance: HMODULE, script: extern "C" fn()) {
        unsafe { (self.register)(instance, script) };
    }

    /// The return buffer belongs to the fiber and is valid until its next native call.
    pub(crate) unsafe fn native(&self, hash: u64, arguments: &[u64]) -> *const u64 {
        unsafe {
            (self.native_init)(hash);
            for &argument in arguments {
                (self.native_push)(argument);
            }
            (self.native_call)()
        }
    }

    pub(crate) unsafe fn wait_frame(&self) {
        unsafe { (self.wait)(0) };
    }
}

impl Input {
    fn invoke(&self, hash: u64, arguments: &[u64]) -> u64 {
        unsafe { self.script_hook.native(hash, arguments).as_ref() }
            .copied()
            .unwrap_or(0)
    }
}

fn foreground_owned() -> bool {
    let mut process = 0;
    unsafe { GetWindowThreadProcessId(GetForegroundWindow(), &mut process) };
    process == unsafe { GetCurrentProcessId() }
}

extern "C" fn script() {
    let Some(input) = INPUT.get() else { return };
    let mut previous_key = false;
    let mut previous_chord = false;
    let mut settings_tick = 0;
    loop {
        let now = unsafe { GetTickCount64() };
        if let Some(settings_get) = input.settings_get
            && now >= settings_tick
        {
            settings_tick = now.wrapping_add(250);
            let mut preferences: rsf_render_settings = unsafe { std::mem::zeroed() };
            preferences.struct_size = size_of::<rsf_render_settings>() as u32;
            preferences.abi_version = RSF_RENDER_FRONTEND_ABI_VERSION;
            if unsafe { settings_get(&mut preferences) } == OK {
                unsafe { quality_tick(preferences.sr_quality) };
            }
        }
        if foreground_owned() {
            let key = unsafe { GetAsyncKeyState(i32::from(VK_F7)) } < 0;
            let chord = input.invoke(CONTROL_PRESSED, &[0, LEFT_STICK]) != 0
                && input.invoke(CONTROL_PRESSED, &[0, RIGHT_STICK]) != 0;
            if (key && !previous_key) || (chord && !previous_chord) {
                unsafe { (input.toggle)() };
            }
            previous_key = key;
            previous_chord = chord;
            // The Runtime owns overlay input and drawing. The game suppresses background actions.
            if unsafe { (input.visible)() } != 0 {
                input.invoke(DISABLE_CONTROL_ACTIONS, &[0]);
            }
        } else {
            previous_key = false;
            previous_chord = false;
        }
        unsafe { input.script_hook.wait_frame() };
    }
}

fn bindings(library: HMODULE, runtime: HMODULE) -> Option<Input> {
    unsafe {
        Some(Input {
            script_hook: ScriptHook::resolve(library)?,
            visible: typed_export(runtime, c"rsf_render_overlay_visible")?,
            toggle: typed_export(runtime, c"rsf_render_overlay_toggle")?,
            settings_get: typed_export(runtime, c"rsf_render_settings_get"),
        })
    }
}

pub(crate) fn start(instance: HMODULE, runtime: HMODULE, configuration: &[u16]) -> bool {
    if instance.is_null() || runtime.is_null() || INPUT.get().is_some() {
        return false;
    }
    let mut library = unsafe { GetModuleHandleW(w!("ScriptHookRDR2.dll")) };
    if library.is_null()
        && let Some(separator) = configuration
            .iter()
            .rposition(|&unit| unit == 0x5c || unit == 0x2f)
    {
        let path = append_path(&configuration[..=separator], "ScriptHookRDR2.dll");
        library = unsafe { LoadLibraryW(path.as_ptr()) };
    }
    if library.is_null() {
        return false;
    }
    let Some(input) = bindings(library, runtime) else {
        return false;
    };
    // The callback can run as soon as the script is registered. Publish its complete table first.
    let script_hook = input.script_hook;
    if INPUT.set(input).is_err() {
        return false;
    }
    unsafe { script_hook.register(instance, script) };
    log(
        "RDR2 shared overlay input guard registered; Insert / F7 / both sticks opens the RSF panel.",
    );
    true
}

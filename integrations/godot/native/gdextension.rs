// SPDX-License-Identifier: MIT
//! The GDExtension entry and its two scene classes, implemented on Godot's C interface
//! (`gdextension_interface.h`) instead of godot-cpp: `ReScaleFrameEffect` (a `CompositorEffect`
//! whose `_render_callback` feeds `effect.rs`) and `ReScaleFrameRuntime` (a `RefCounted` whose
//! methods reach `extension.rs` and the Runtime control). The interface functions, the method
//! hashes and the builtin sizes are those of the pinned Godot 4.7 API, which the add-on's
//! `compatibility_minimum` requires.
mod api;
pub(crate) mod effect;
mod runtime;
mod sys;

use crate::properties::Property;
use api::{Dictionary, GString, Godot, Interface, StringName, Variant};
use std::ffi::{CStr, c_void};
use sys::*;

/// The API baseline: godot-cpp refuses an engine older than the one its bindings target.
const MINIMUM_MINOR: u32 = 7;

/// `rsf_godot_initialize`, the add-on's `entry_symbol`.
///
/// # Safety
/// Godot calls this once with its interface resolver, the library token and the initialization
/// record to fill.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_godot_initialize(
    get_proc_address: Option<GetProcAddress>,
    library: *mut c_void,
    initialization: *mut Initialization,
) -> GdBool {
    let (Some(get), Some(initialization)) = (get_proc_address, unsafe { initialization.as_mut() })
    else {
        return 0;
    };
    let Some(interface) = (unsafe { Interface::load(get) }) else {
        return 0;
    };
    let interface = api::publish_interface(interface, library);
    if !compatible(interface) {
        unsafe {
            (interface.print_error)(
                c"ReScaleFrame requires Godot 4.7 or later in the 4.x series.".as_ptr(),
                c"rsf_godot_initialize".as_ptr(),
                c"gdextension.rs".as_ptr(),
                0,
                0,
            )
        };
        return 0;
    }
    *initialization = Initialization {
        minimum_initialization_level: INITIALIZATION_SCENE,
        userdata: std::ptr::null_mut(),
        initialize: Some(initialize),
        deinitialize: Some(deinitialize),
    };
    1
}

fn compatible(interface: &Interface) -> bool {
    let mut version = GodotVersion2 {
        major: 0,
        minor: 0,
        patch: 0,
        hex: 0,
        status: std::ptr::null(),
        build: std::ptr::null(),
        hash: std::ptr::null(),
        timestamp: 0,
        string: std::ptr::null(),
    };
    unsafe { (interface.get_godot_version2)(&mut version) };
    version.major == 4 && version.minor >= MINIMUM_MINOR
}

const EFFECT_CLASS: &CStr = c"ReScaleFrameEffect";
const RUNTIME_CLASS: &CStr = c"ReScaleFrameRuntime";

unsafe extern "C" fn initialize(_: *mut c_void, level: u32) {
    if level != INITIALIZATION_SCENE {
        return;
    }
    let Some(godot) = api::resolve() else {
        if let Some(interface) = api::interface() {
            unsafe {
                (interface.print_error)(
                    c"ReScaleFrame could not resolve the Godot builtin types.".as_ptr(),
                    c"initialize".as_ptr(),
                    c"gdextension.rs".as_ptr(),
                    0,
                    0,
                )
            };
        }
        return;
    };
    register_class(
        godot,
        EFFECT_CLASS,
        c"CompositorEffect",
        effect::create,
        effect::free,
        Some(effect::get_virtual),
    );
    for method in [Method::EffectGetStatus, Method::EffectRequestCapture] {
        register_method(godot, EFFECT_CLASS, method);
    }
    register_class(
        godot,
        RUNTIME_CLASS,
        c"RefCounted",
        runtime::create,
        runtime::free,
        None,
    );
    for method in [
        Method::RuntimeConfigure,
        Method::RuntimeGetStatus,
        Method::RuntimeResetHistory,
        Method::RuntimeRequestLatencyPing,
        Method::RuntimeRequestFrameDump,
    ] {
        register_method(godot, RUNTIME_CLASS, method);
    }
}

unsafe extern "C" fn deinitialize(_: *mut c_void, level: u32) {
    if level != INITIALIZATION_SCENE {
        return;
    }
    if let Some(godot) = api::godot() {
        // Reverse registration order, as godot-cpp's ClassDB unregisters.
        for class in [RUNTIME_CLASS, EFFECT_CLASS] {
            let class = StringName::new(godot, class);
            unsafe {
                (godot.interface.classdb_unregister_extension_class)(godot.library, class.ptr())
            };
        }
    }
}

fn register_class(
    godot: &Godot,
    class: &CStr,
    parent: &CStr,
    create: CreateInstance3,
    free: FreeInstance,
    get_virtual: Option<GetVirtual2>,
) {
    let info = ClassCreationInfo6 {
        is_virtual: 0,
        is_abstract: 0,
        is_exposed: 1,
        is_runtime: 0,
        icon_path: std::ptr::null(),
        unused_callbacks: [std::ptr::null(); 11],
        create_instance_func: Some(create),
        free_instance_func: Some(free),
        recreate_instance_func: std::ptr::null(),
        get_virtual_func: get_virtual,
        get_virtual_call_data_func: std::ptr::null(),
        call_virtual_with_data_func: std::ptr::null(),
        class_userdata: std::ptr::null_mut(),
    };
    let class = StringName::new(godot, class);
    let parent = StringName::new(godot, parent);
    unsafe {
        (godot.interface.classdb_register_extension_class6)(
            godot.library,
            class.ptr(),
            parent.ptr(),
            &info,
        )
    };
}

/// Every bound method; the discriminant is the bind's `method_userdata`.
#[derive(Clone, Copy, PartialEq)]
#[repr(usize)]
enum Method {
    EffectGetStatus,
    EffectRequestCapture,
    RuntimeConfigure,
    RuntimeGetStatus,
    RuntimeResetHistory,
    RuntimeRequestLatencyPing,
    RuntimeRequestFrameDump,
}

const METHODS: [Method; 7] = [
    Method::EffectGetStatus,
    Method::EffectRequestCapture,
    Method::RuntimeConfigure,
    Method::RuntimeGetStatus,
    Method::RuntimeResetHistory,
    Method::RuntimeRequestLatencyPing,
    Method::RuntimeRequestFrameDump,
];

impl Method {
    fn of(userdata: *mut c_void) -> Option<Self> {
        METHODS.get(userdata as usize).copied()
    }

    /// The script name, the Variant return type and the named argument types.
    fn signature(
        self,
    ) -> (
        &'static CStr,
        VariantType,
        &'static [(&'static CStr, VariantType)],
    ) {
        match self {
            Self::EffectGetStatus | Self::RuntimeGetStatus => (c"get_status", TYPE_DICTIONARY, &[]),
            Self::EffectRequestCapture => (c"request_motion_capture", TYPE_NIL, &[]),
            Self::RuntimeConfigure => (
                c"configure",
                TYPE_BOOL,
                &[(c"options", TYPE_DICTIONARY), (c"viewport", TYPE_RID)],
            ),
            Self::RuntimeResetHistory => (c"reset_history", TYPE_NIL, &[]),
            Self::RuntimeRequestLatencyPing => (c"request_latency_ping", TYPE_BOOL, &[]),
            Self::RuntimeRequestFrameDump => (c"request_frame_dump", TYPE_NIL, &[]),
        }
    }

    /// The two `get_status` methods are const in their godot-cpp classes.
    fn flags(self) -> u32 {
        match self {
            Self::EffectGetStatus | Self::RuntimeGetStatus => {
                METHOD_FLAG_NORMAL | METHOD_FLAG_CONST
            }
            _ => METHOD_FLAG_NORMAL,
        }
    }
}

fn property(
    kind: VariantType,
    name: &mut StringName,
    class: &mut StringName,
    hint: &mut GString,
) -> PropertyInfo {
    PropertyInfo {
        kind,
        name: name.ptr_mut(),
        class_name: class.ptr_mut(),
        hint: 0,
        hint_string: hint.ptr_mut(),
        usage: PROPERTY_USAGE_DEFAULT,
    }
}

fn register_method(godot: &'static Godot, class: &CStr, method: Method) {
    let (name, returns, arguments) = method.signature();
    let mut name = StringName::new(godot, name);
    let mut empty_name = StringName::new(godot, c"");
    let mut empty_hint = GString::empty(godot);
    let mut return_name = StringName::new(godot, c"");
    let mut return_info = property(returns, &mut return_name, &mut empty_name, &mut empty_hint);
    let mut argument_names: Vec<StringName> = arguments
        .iter()
        .map(|(argument, _)| StringName::new(godot, argument))
        .collect();
    let mut argument_infos: Vec<PropertyInfo> = arguments
        .iter()
        .zip(argument_names.iter_mut())
        .map(|((_, kind), argument)| PropertyInfo {
            kind: *kind,
            name: argument.ptr_mut(),
            class_name: empty_name.ptr_mut(),
            hint: 0,
            hint_string: empty_hint.ptr_mut(),
            usage: PROPERTY_USAGE_DEFAULT,
        })
        .collect();
    let mut metadata = vec![0u32; arguments.len()];
    let info = ClassMethodInfo {
        name: name.ptr_mut(),
        method_userdata: method as usize as *mut c_void,
        call_func: Some(call),
        ptrcall_func: Some(ptrcall),
        method_flags: method.flags(),
        has_return_value: u8::from(returns != TYPE_NIL),
        return_value_info: &mut return_info,
        return_value_metadata: 0,
        argument_count: arguments.len() as u32,
        arguments_info: argument_infos.as_mut_ptr(),
        arguments_metadata: metadata.as_mut_ptr(),
        default_argument_count: 0,
        default_arguments: std::ptr::null_mut(),
    };
    let class = StringName::new(godot, class);
    unsafe {
        (godot.interface.classdb_register_extension_class_method)(godot.library, class.ptr(), &info)
    };
}

/// The typed argument and return values shared by the Variant and pointer calls.
enum Returned {
    Nothing,
    Boolean(bool),
    Dictionary(Dictionary),
}

/// Runs `method` on its instance with already converted arguments.
///
/// # Safety
/// `instance` is the method's class instance; `options` is a live Dictionary for `configure`.
unsafe fn invoke(
    godot: &'static Godot,
    method: Method,
    instance: *mut c_void,
    options: ConstTypePtr,
    viewport: u64,
) -> Returned {
    match method {
        Method::EffectGetStatus => {
            Returned::Dictionary(unsafe { effect::get_status(godot, instance) })
        }
        Method::EffectRequestCapture => {
            unsafe { effect::request_motion_capture(instance) };
            Returned::Nothing
        }
        Method::RuntimeConfigure => {
            Returned::Boolean(unsafe { runtime::configure(godot, options, viewport) })
        }
        Method::RuntimeGetStatus => Returned::Dictionary(runtime::get_status(godot)),
        Method::RuntimeResetHistory => {
            runtime::reset_history();
            Returned::Nothing
        }
        Method::RuntimeRequestLatencyPing => Returned::Boolean(runtime::request_latency_ping()),
        Method::RuntimeRequestFrameDump => {
            runtime::request_frame_dump(godot);
            Returned::Nothing
        }
    }
}

fn instance_required(method: Method) -> bool {
    matches!(
        method,
        Method::EffectGetStatus | Method::EffectRequestCapture
    )
}

/// `GDExtensionClassMethodCall`: godot-cpp's argument count and strict conversion checks, then
/// the Variant return.
unsafe extern "C" fn call(
    userdata: *mut c_void,
    instance: *mut c_void,
    args: *const *const c_void,
    count: GdInt,
    ret: *mut c_void,
    error: *mut CallError,
) {
    let (Some(godot), Some(method), Some(error)) = (api::godot(), Method::of(userdata), unsafe {
        error.as_mut()
    }) else {
        return;
    };
    *error = CallError {
        error: CALL_OK,
        argument: 0,
        expected: 0,
    };
    let (_, _, arguments) = method.signature();
    let expected = arguments.len() as GdInt;
    if count > expected {
        *error = CallError {
            error: CALL_ERROR_TOO_MANY_ARGUMENTS,
            argument: 0,
            expected: expected as i32,
        };
        return;
    }
    if count < expected {
        *error = CallError {
            error: CALL_ERROR_TOO_FEW_ARGUMENTS,
            argument: 0,
            expected: expected as i32,
        };
        return;
    }
    if instance_required(method) && instance.is_null() {
        *error = CallError {
            error: CALL_ERROR_INSTANCE_IS_NULL,
            argument: 0,
            expected: 0,
        };
        return;
    }
    let argument = |index: usize| unsafe { *args.add(index) };
    for (index, (_, kind)) in arguments.iter().enumerate() {
        let actual = unsafe { (godot.interface.variant_get_type)(argument(index)) };
        if unsafe { (godot.interface.variant_can_convert_strict)(actual, *kind) } == 0 {
            *error = CallError {
                error: CALL_ERROR_INVALID_ARGUMENT,
                argument: index as i32,
                expected: *kind as i32,
            };
            return;
        }
    }
    let (options, viewport) = if method == Method::RuntimeConfigure {
        let options = unsafe { Dictionary::of_variant(godot, argument(0)) };
        let mut viewport = 0u64;
        unsafe { (godot.to_rid)((&raw mut viewport).cast(), argument(1).cast_mut()) };
        (Some(options), viewport)
    } else {
        (None, 0)
    };
    let options_ptr = options.as_ref().map_or(std::ptr::null(), Dictionary::ptr);
    match unsafe { invoke(godot, method, instance, options_ptr, viewport) } {
        Returned::Nothing => {}
        Returned::Boolean(value) => {
            let value = u8::from(value);
            unsafe { api::assign(godot, ret, godot.from_bool, api::arg(&value)) };
        }
        Returned::Dictionary(value) => unsafe {
            api::assign(godot, ret, godot.from_dictionary, value.ptr())
        },
    }
}

/// `GDExtensionClassMethodPtrCall`: arguments and the constructed return by pointer.
unsafe extern "C" fn ptrcall(
    userdata: *mut c_void,
    instance: *mut c_void,
    args: *const ConstTypePtr,
    ret: TypePtr,
) {
    let (Some(godot), Some(method)) = (api::godot(), Method::of(userdata)) else {
        return;
    };
    if instance_required(method) && instance.is_null() {
        return;
    }
    let (options, viewport) = if method == Method::RuntimeConfigure {
        unsafe { (*args, (*args.add(1)).cast::<u64>().read_unaligned()) }
    } else {
        (std::ptr::null(), 0)
    };
    match unsafe { invoke(godot, method, instance, options, viewport) } {
        Returned::Nothing => {}
        Returned::Boolean(value) => unsafe { ret.cast::<u8>().write(u8::from(value)) },
        Returned::Dictionary(value) => unsafe { value.assign_to(ret) },
    }
}

/// The property sink of `ScriptDictionary`: each `Property` becomes a Dictionary entry with
/// godot-cpp's Variant conversions.
struct Sink<'a> {
    godot: &'static Godot,
    dictionary: &'a mut Dictionary,
    rendering: ObjectPtr,
}

impl Sink<'_> {
    fn put(&mut self, key: &CStr, property: Property) {
        let godot = self.godot;
        let key = Variant::string(godot, key);
        let slot = unsafe { api::dictionary_slot(godot, self.dictionary.ptr_mut(), &key) };
        if slot.is_null() {
            return;
        }
        let put = |from, value: ConstTypePtr| unsafe { api::assign(godot, slot, from, value) };
        match property {
            Property::Integer(value) => put(godot.from_int, api::arg(&value)),
            Property::Bool(value) => put(godot.from_bool, api::arg(&u8::from(value))),
            Property::Real(value) => put(godot.from_float, api::arg(&value)),
            Property::Text(text) => {
                let text = GString::new(godot, text);
                put(godot.from_string, text.ptr());
            }
            Property::Extent(extent) => put(godot.from_vector2i, api::arg(&extent)),
            Property::Transform(matrix) => {
                // Basis rows then the origin: Transform3D's single-precision memory order.
                let transform: [f32; 12] = matrix.map(|value| value as f32);
                put(godot.from_transform3d, api::arg(&transform));
            }
            Property::Projection(matrix) => {
                // Columns: Projection's single-precision memory order.
                let projection: [f32; 16] = matrix.map(|value| value as f32);
                put(godot.from_projection, api::arg(&projection));
            }
            Property::Renderer | Property::Driver => {
                let method = if matches!(property, Property::Renderer) {
                    api::Bind::GET_RENDERING_METHOD
                } else {
                    api::Bind::GET_RENDERING_DRIVER_NAME
                };
                let name = if self.rendering.is_null() {
                    None
                } else {
                    unsafe { godot.string(method, self.rendering, &[]) }
                };
                let name = name.unwrap_or_else(|| GString::empty(godot));
                put(godot.from_string, name.ptr());
            }
        }
    }
}

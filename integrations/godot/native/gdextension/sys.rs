// SPDX-License-Identifier: MIT
//! The GDExtension C interface records, function types and constants this extension uses, from
//! the pinned Godot 4.7 `gdextension_interface.json` and `extension_api-4-7.json` (single
//! precision, x86_64). Builtin values are opaque byte blocks of the API's `float_64` sizes.
use std::ffi::{c_char, c_void};

pub(crate) type ObjectPtr = *mut c_void;
pub(crate) type MethodBindPtr = *const c_void;
pub(crate) type TypePtr = *mut c_void;
pub(crate) type ConstTypePtr = *const c_void;
/// `GDExtensionVariantType`, a C enum.
pub(crate) type VariantType = u32;
pub(crate) type GdBool = u8;
pub(crate) type GdInt = i64;

pub(crate) const TYPE_NIL: VariantType = 0;
pub(crate) const TYPE_BOOL: VariantType = 1;
pub(crate) const TYPE_INT: VariantType = 2;
pub(crate) const TYPE_FLOAT: VariantType = 3;
pub(crate) const TYPE_STRING: VariantType = 4;
pub(crate) const TYPE_VECTOR2I: VariantType = 6;
pub(crate) const TYPE_TRANSFORM3D: VariantType = 18;
pub(crate) const TYPE_PROJECTION: VariantType = 19;
pub(crate) const TYPE_STRING_NAME: VariantType = 21;
pub(crate) const TYPE_RID: VariantType = 23;
pub(crate) const TYPE_OBJECT: VariantType = 24;
pub(crate) const TYPE_CALLABLE: VariantType = 25;
pub(crate) const TYPE_DICTIONARY: VariantType = 27;
pub(crate) const TYPE_ARRAY: VariantType = 28;
pub(crate) const TYPE_PACKED_BYTE_ARRAY: VariantType = 29;

pub(crate) const VARIANT_SIZE: usize = 24;
pub(crate) const STRING_SIZE: usize = 8;
pub(crate) const STRING_NAME_SIZE: usize = 8;
pub(crate) const DICTIONARY_SIZE: usize = 8;
pub(crate) const ARRAY_SIZE: usize = 8;
pub(crate) const PACKED_BYTE_ARRAY_SIZE: usize = 16;
pub(crate) const CALLABLE_SIZE: usize = 16;

pub(crate) const INITIALIZATION_SCENE: u32 = 2;
pub(crate) const METHOD_FLAG_NORMAL: u32 = 1;
pub(crate) const METHOD_FLAG_CONST: u32 = 4;
pub(crate) const CALL_OK: u32 = 0;
pub(crate) const CALL_ERROR_INVALID_ARGUMENT: u32 = 2;
pub(crate) const CALL_ERROR_TOO_MANY_ARGUMENTS: u32 = 3;
pub(crate) const CALL_ERROR_TOO_FEW_ARGUMENTS: u32 = 4;
pub(crate) const CALL_ERROR_INSTANCE_IS_NULL: u32 = 5;
/// `PROPERTY_USAGE_DEFAULT`, godot-cpp's `PropertyInfo` default.
pub(crate) const PROPERTY_USAGE_DEFAULT: u32 = 6;
pub(crate) const NOTIFICATION_POSTINITIALIZE: i64 = 0;

/// An opaque builtin value with Godot's size and pointer alignment.
#[repr(C, align(8))]
pub(crate) struct Opaque<const N: usize>(pub(crate) [u8; N]);

impl<const N: usize> Opaque<N> {
    pub(crate) const fn zeroed() -> Self {
        Self([0; N])
    }
}

pub(crate) type InterfaceFunction = unsafe extern "C" fn();
pub(crate) type GetProcAddress = unsafe extern "C" fn(*const c_char) -> Option<InterfaceFunction>;

pub(crate) type PtrConstructor = unsafe extern "C" fn(TypePtr, *const ConstTypePtr);
pub(crate) type PtrDestructor = unsafe extern "C" fn(TypePtr);
pub(crate) type PtrBuiltinMethod = unsafe extern "C" fn(TypePtr, *const ConstTypePtr, TypePtr, i32);
pub(crate) type FromTypeConstructor = unsafe extern "C" fn(*mut c_void, TypePtr);
pub(crate) type ToTypeConstructor = unsafe extern "C" fn(*mut c_void, *mut c_void);

pub(crate) type MethodCall = unsafe extern "C" fn(
    *mut c_void,
    *mut c_void,
    *const *const c_void,
    GdInt,
    *mut c_void,
    *mut CallError,
);
pub(crate) type MethodPtrCall =
    unsafe extern "C" fn(*mut c_void, *mut c_void, *const ConstTypePtr, TypePtr);
pub(crate) type CreateInstance3 = unsafe extern "C" fn(*mut c_void, GdBool) -> ObjectPtr;
pub(crate) type FreeInstance = unsafe extern "C" fn(*mut c_void, *mut c_void);
pub(crate) type CallVirtual = unsafe extern "C" fn(*mut c_void, *const ConstTypePtr, TypePtr);
pub(crate) type GetVirtual2 =
    unsafe extern "C" fn(*mut c_void, *const c_void, u32) -> Option<CallVirtual>;
pub(crate) type CallableCall =
    unsafe extern "C" fn(*mut c_void, *const *const c_void, GdInt, *mut c_void, *mut CallError);
pub(crate) type CallableArgumentCount = unsafe extern "C" fn(*mut c_void, *mut GdBool) -> GdInt;
pub(crate) type InitializeLevel = unsafe extern "C" fn(*mut c_void, u32);

/// `GDExtensionInitialization`.
#[repr(C)]
pub(crate) struct Initialization {
    pub(crate) minimum_initialization_level: u32,
    pub(crate) userdata: *mut c_void,
    pub(crate) initialize: Option<InitializeLevel>,
    pub(crate) deinitialize: Option<InitializeLevel>,
}

/// `GDExtensionGodotVersion2`.
#[repr(C)]
pub(crate) struct GodotVersion2 {
    pub(crate) major: u32,
    pub(crate) minor: u32,
    pub(crate) patch: u32,
    pub(crate) hex: u32,
    pub(crate) status: *const c_char,
    pub(crate) build: *const c_char,
    pub(crate) hash: *const c_char,
    pub(crate) timestamp: u64,
    pub(crate) string: *const c_char,
}

/// `GDExtensionCallError`.
#[repr(C)]
pub(crate) struct CallError {
    pub(crate) error: u32,
    pub(crate) argument: i32,
    pub(crate) expected: i32,
}

/// `GDExtensionPropertyInfo`.
#[repr(C)]
pub(crate) struct PropertyInfo {
    pub(crate) kind: VariantType,
    pub(crate) name: *mut c_void,
    pub(crate) class_name: *mut c_void,
    pub(crate) hint: u32,
    pub(crate) hint_string: *mut c_void,
    pub(crate) usage: u32,
}

/// `GDExtensionClassMethodInfo`.
#[repr(C)]
pub(crate) struct ClassMethodInfo {
    pub(crate) name: *mut c_void,
    pub(crate) method_userdata: *mut c_void,
    pub(crate) call_func: Option<MethodCall>,
    pub(crate) ptrcall_func: Option<MethodPtrCall>,
    pub(crate) method_flags: u32,
    pub(crate) has_return_value: GdBool,
    pub(crate) return_value_info: *mut PropertyInfo,
    /// `GDExtensionClassMethodArgumentMetadata`.
    pub(crate) return_value_metadata: u32,
    pub(crate) argument_count: u32,
    pub(crate) arguments_info: *mut PropertyInfo,
    pub(crate) arguments_metadata: *mut u32,
    pub(crate) default_argument_count: u32,
    pub(crate) default_arguments: *mut *mut c_void,
}

/// `GDExtensionClassCreationInfo6`. Optional callbacks this extension does not implement are
/// null data-sized slots.
#[repr(C)]
pub(crate) struct ClassCreationInfo6 {
    pub(crate) is_virtual: GdBool,
    pub(crate) is_abstract: GdBool,
    pub(crate) is_exposed: GdBool,
    pub(crate) is_runtime: GdBool,
    pub(crate) icon_path: *const c_void,
    /// set, get, get_property_list, free_property_list, property_can_revert,
    /// property_get_revert, validate_property, notification, to_string, reference, unreference.
    pub(crate) unused_callbacks: [*const c_void; 11],
    pub(crate) create_instance_func: Option<CreateInstance3>,
    pub(crate) free_instance_func: Option<FreeInstance>,
    pub(crate) recreate_instance_func: *const c_void,
    pub(crate) get_virtual_func: Option<GetVirtual2>,
    pub(crate) get_virtual_call_data_func: *const c_void,
    pub(crate) call_virtual_with_data_func: *const c_void,
    pub(crate) class_userdata: *mut c_void,
}

/// `GDExtensionCallableCustomInfo2`.
#[repr(C)]
pub(crate) struct CallableCustomInfo2 {
    pub(crate) callable_userdata: *mut c_void,
    pub(crate) token: *mut c_void,
    pub(crate) object_id: u64,
    pub(crate) call_func: Option<CallableCall>,
    /// is_valid, free, hash, equal, less_than, to_string.
    pub(crate) unused_callbacks: [*const c_void; 6],
    pub(crate) get_argument_count_func: Option<CallableArgumentCount>,
}

const _: () = {
    assert!(size_of::<Initialization>() == 32);
    assert!(size_of::<GodotVersion2>() == 56);
    assert!(size_of::<CallError>() == 12);
    assert!(size_of::<PropertyInfo>() == 48);
    assert!(size_of::<ClassMethodInfo>() == 88);
    assert!(size_of::<ClassCreationInfo6>() == 160);
    assert!(size_of::<CallableCustomInfo2>() == 88);
};

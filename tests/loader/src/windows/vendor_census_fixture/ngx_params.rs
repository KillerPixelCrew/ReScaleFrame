// SPDX-License-Identifier: MIT
use super::{Ptr, lock};
use std::collections::BTreeMap;
use std::ffi::{CStr, c_char};
use std::sync::Mutex;

const SUCCESS: i32 = 1;
const BAD_PARAMETER: i32 = 0xbad0_0000u32 as i32;

enum Value {
    Number(f64),
    Pointer(usize),
}

#[repr(C)]
struct Parameters {
    vtable: &'static Vtable,
    values: Mutex<BTreeMap<Vec<u8>, Value>>,
}

type Set<T> = unsafe extern "system" fn(&Parameters, *const c_char, T);
type Get<T> = unsafe extern "system" fn(&Parameters, *const c_char, *mut T) -> i32;

// MSVC reverses the overloads within each named group in the native fixture declaration.
#[repr(C)]
struct Vtable {
    set_void: Set<Ptr>,
    set_d3d12: Set<Ptr>,
    set_d3d11: Set<Ptr>,
    set_int: Set<i32>,
    set_uint: Set<u32>,
    set_double: Set<f64>,
    set_float: Set<f32>,
    set_ull: Set<u64>,
    get_void: Get<Ptr>,
    get_d3d12: Get<Ptr>,
    get_d3d11: Get<Ptr>,
    get_int: Get<i32>,
    get_uint: Get<u32>,
    get_double: Get<f64>,
    get_float: Get<f32>,
    get_ull: Get<u64>,
    reset: extern "system" fn(&Parameters),
}

macro_rules! numeric_accessors {
    ($set:ident, $get:ident, $ty:ty) => {
        unsafe extern "system" fn $set(params: &Parameters, name: *const c_char, value: $ty) {
            let name = unsafe { CStr::from_ptr(name) }.to_bytes();
            lock(&params.values).insert(name.to_vec(), Value::Number(value as f64));
        }

        unsafe extern "system" fn $get(
            params: &Parameters,
            name: *const c_char,
            out: *mut $ty,
        ) -> i32 {
            let name = unsafe { CStr::from_ptr(name) }.to_bytes();
            if let Some(Value::Number(value)) = lock(&params.values).get(name) {
                unsafe { out.write(*value as $ty) };
                SUCCESS
            } else {
                BAD_PARAMETER
            }
        }
    };
}

numeric_accessors!(set_int, get_int, i32);
numeric_accessors!(set_uint, get_uint, u32);
numeric_accessors!(set_double, get_double, f64);
numeric_accessors!(set_float, get_float, f32);
numeric_accessors!(set_ull, get_ull, u64);

unsafe extern "system" fn set_pointer(params: &Parameters, name: *const c_char, value: Ptr) {
    let name = unsafe { CStr::from_ptr(name) }.to_bytes();
    lock(&params.values).insert(name.to_vec(), Value::Pointer(value as usize));
}

unsafe extern "system" fn get_pointer(
    params: &Parameters,
    name: *const c_char,
    out: *mut Ptr,
) -> i32 {
    let name = unsafe { CStr::from_ptr(name) }.to_bytes();
    if let Some(Value::Pointer(value)) = lock(&params.values).get(name) {
        unsafe { out.write(*value as Ptr) };
        SUCCESS
    } else {
        BAD_PARAMETER
    }
}

extern "system" fn reset(params: &Parameters) {
    lock(&params.values).clear();
}

static VTABLE: Vtable = Vtable {
    set_void: set_pointer,
    set_d3d12: set_pointer,
    set_d3d11: set_pointer,
    set_int,
    set_uint,
    set_double,
    set_float,
    set_ull,
    get_void: get_pointer,
    get_d3d12: get_pointer,
    get_d3d11: get_pointer,
    get_int,
    get_uint,
    get_double,
    get_float,
    get_ull,
    reset,
};

static PARAMETERS: Parameters = Parameters {
    vtable: &VTABLE,
    values: Mutex::new(BTreeMap::new()),
};

#[unsafe(no_mangle)]
unsafe extern "C" fn NVSDK_NGX_D3D12_GetParameters(out: *mut Ptr) -> i32 {
    unsafe { out.write((&raw const PARAMETERS).cast_mut().cast()) };
    SUCCESS
}

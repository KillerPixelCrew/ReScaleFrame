// SPDX-License-Identifier: MIT
//! The loaded GDExtension interface, the builtin constructors and the engine method binds, with
//! owning wrappers for the builtin values and RefCounted references this extension holds.
//!
//! Ptrcall conventions (Godot `method_ptrcall.h`): integers and enums travel as `i64`, booleans as
//! one byte, a RID as its `u64` id, an object as a pointer to its object pointer, and a builtin by
//! pointer. Returned builtins are assigned, so their storage is constructed first. A returned
//! `Ref<T>` is written as one owned reference.
use super::sys::*;
use std::ffi::{CStr, c_char, c_void};
use std::sync::OnceLock;

macro_rules! interface {
    ($($field:ident: $ty:ty,)*) => {
        /// The interface functions, resolved by name when the library is loaded.
        pub(crate) struct Interface {
            $(pub(crate) $field: $ty,)*
        }

        impl Interface {
            /// # Safety
            /// `get` is Godot's `GDExtensionInterfaceGetProcAddress`.
            pub(crate) unsafe fn load(get: GetProcAddress) -> Option<Self> {
                Some(Self {
                    $($field: {
                        let name = concat!(stringify!($field), "\0");
                        let function = unsafe { get(name.as_ptr().cast()) }?;
                        // SAFETY: the interface documents this function's signature as `$ty`.
                        unsafe { std::mem::transmute::<InterfaceFunction, $ty>(function) }
                    },)*
                })
            }
        }
    };
}

interface! {
    get_godot_version2: unsafe extern "C" fn(*mut GodotVersion2),
    print_error: unsafe extern "C" fn(*const c_char, *const c_char, *const c_char, i32, GdBool),
    classdb_register_extension_class6:
        unsafe extern "C" fn(*mut c_void, *const c_void, *const c_void, *const ClassCreationInfo6),
    classdb_register_extension_class_method:
        unsafe extern "C" fn(*mut c_void, *const c_void, *const ClassMethodInfo),
    classdb_unregister_extension_class: unsafe extern "C" fn(*mut c_void, *const c_void),
    classdb_construct_object3: unsafe extern "C" fn(*const c_void) -> ObjectPtr,
    classdb_get_method_bind: unsafe extern "C" fn(*const c_void, *const c_void, GdInt) -> MethodBindPtr,
    object_set_instance: unsafe extern "C" fn(ObjectPtr, *const c_void, *mut c_void),
    object_destroy: unsafe extern "C" fn(ObjectPtr),
    object_method_bind_ptrcall:
        unsafe extern "C" fn(MethodBindPtr, ObjectPtr, *const ConstTypePtr, TypePtr),
    global_get_singleton: unsafe extern "C" fn(*const c_void) -> ObjectPtr,
    string_name_new_with_latin1_chars: unsafe extern "C" fn(*mut c_void, *const c_char, GdBool),
    string_new_with_utf8_chars: unsafe extern "C" fn(*mut c_void, *const c_char),
    string_to_utf8_chars: unsafe extern "C" fn(*const c_void, *mut c_char, GdInt) -> GdInt,
    variant_get_ptr_constructor: unsafe extern "C" fn(VariantType, i32) -> Option<PtrConstructor>,
    variant_get_ptr_destructor: unsafe extern "C" fn(VariantType) -> Option<PtrDestructor>,
    variant_get_ptr_builtin_method:
        unsafe extern "C" fn(VariantType, *const c_void, GdInt) -> Option<PtrBuiltinMethod>,
    get_variant_from_type_constructor: unsafe extern "C" fn(VariantType) -> Option<FromTypeConstructor>,
    get_variant_to_type_constructor: unsafe extern "C" fn(VariantType) -> Option<ToTypeConstructor>,
    variant_get_type: unsafe extern "C" fn(*const c_void) -> VariantType,
    variant_can_convert_strict: unsafe extern "C" fn(VariantType, VariantType) -> GdBool,
    variant_destroy: unsafe extern "C" fn(*mut c_void),
    dictionary_operator_index: unsafe extern "C" fn(TypePtr, *const c_void) -> *mut c_void,
    dictionary_operator_index_const: unsafe extern "C" fn(ConstTypePtr, *const c_void) -> *mut c_void,
    packed_byte_array_operator_index_const: unsafe extern "C" fn(ConstTypePtr, GdInt) -> *const u8,
    array_set_typed: unsafe extern "C" fn(TypePtr, VariantType, *const c_void, *const c_void),
    callable_custom_create2: unsafe extern "C" fn(*mut c_void, *mut CallableCustomInfo2),
}

macro_rules! methods {
    ($($id:ident = $class:literal . $method:literal # $hash:literal;)*) => {
        /// An engine method bind, by its index in `METHODS`.
        #[allow(non_camel_case_types, clippy::upper_case_acronyms)]
        #[derive(Clone, Copy)]
        #[repr(usize)]
        pub(crate) enum Bind { $($id,)* }

        /// Class, method and the pinned 4.7 API hash of each bind.
        const METHODS: &[(&CStr, &CStr, i64)] = &[$(($class, $method, $hash),)*];
    };
}

methods! {
    OBJECT_IS_CLASS = c"Object".c"is_class" # 2619796661;
    OBJECT_NOTIFICATION = c"Object".c"notification" # 4023243586;
    REF_COUNTED_UNREFERENCE = c"RefCounted".c"unreference" # 2240911060;
    SET_EFFECT_CALLBACK_TYPE = c"CompositorEffect".c"set_effect_callback_type" # 1390728419;
    SET_ACCESS_RESOLVED_COLOR = c"CompositorEffect".c"set_access_resolved_color" # 2586408642;
    SET_ACCESS_RESOLVED_DEPTH = c"CompositorEffect".c"set_access_resolved_depth" # 2586408642;
    SET_NEEDS_MOTION_VECTORS = c"CompositorEffect".c"set_needs_motion_vectors" # 2586408642;
    GET_RENDER_SCENE_BUFFERS = c"RenderData".c"get_render_scene_buffers" # 2793216201;
    GET_RENDER_SCENE_DATA = c"RenderData".c"get_render_scene_data" # 1288715698;
    GET_RENDER_TARGET = c"RenderSceneBuffersRD".c"get_render_target" # 2944877500;
    GET_VIEW_COUNT = c"RenderSceneBuffersRD".c"get_view_count" # 3905245786;
    GET_INTERNAL_SIZE = c"RenderSceneBuffersRD".c"get_internal_size" # 3690982128;
    GET_TARGET_SIZE = c"RenderSceneBuffersRD".c"get_target_size" # 3690982128;
    GET_COLOR_LAYER = c"RenderSceneBuffersRD".c"get_color_layer" # 3087988589;
    GET_DEPTH_LAYER = c"RenderSceneBuffersRD".c"get_depth_layer" # 3087988589;
    GET_VELOCITY_LAYER = c"RenderSceneBuffersRD".c"get_velocity_layer" # 3087988589;
    GET_CAM_TRANSFORM = c"RenderSceneData".c"get_cam_transform" # 3229777777;
    GET_CAM_PROJECTION = c"RenderSceneData".c"get_cam_projection" # 2910717950;
    GET_RENDERING_DEVICE = c"RenderingServer".c"get_rendering_device" # 1405107940;
    GET_RENDERING_DRIVER_NAME = c"RenderingServer".c"get_current_rendering_driver_name" # 201670096;
    GET_RENDERING_METHOD = c"RenderingServer".c"get_current_rendering_method" # 201670096;
    CALL_ON_RENDER_THREAD = c"RenderingServer".c"call_on_render_thread" # 1611583062;
    TEXTURE_IS_VALID = c"RenderingDevice".c"texture_is_valid" # 3521089500;
    TEXTURE_GET_FORMAT = c"RenderingDevice".c"texture_get_format" # 1374471690;
    GET_DRIVER_RESOURCE = c"RenderingDevice".c"get_driver_resource" # 501815484;
    SHADER_COMPILE_SPIRV_FROM_SOURCE = c"RenderingDevice".c"shader_compile_spirv_from_source" # 1178973306;
    SHADER_CREATE_FROM_SPIRV = c"RenderingDevice".c"shader_create_from_spirv" # 342949005;
    COMPUTE_PIPELINE_CREATE = c"RenderingDevice".c"compute_pipeline_create" # 1448838280;
    SAMPLER_CREATE = c"RenderingDevice".c"sampler_create" # 2327892535;
    STORAGE_BUFFER_CREATE = c"RenderingDevice".c"storage_buffer_create" # 1609052553;
    UNIFORM_SET_CREATE = c"RenderingDevice".c"uniform_set_create" # 2280795797;
    COMPUTE_LIST_BEGIN = c"RenderingDevice".c"compute_list_begin" # 2455072627;
    COMPUTE_LIST_BIND_COMPUTE_PIPELINE = c"RenderingDevice".c"compute_list_bind_compute_pipeline" # 4040184819;
    COMPUTE_LIST_BIND_UNIFORM_SET = c"RenderingDevice".c"compute_list_bind_uniform_set" # 749655778;
    COMPUTE_LIST_DISPATCH = c"RenderingDevice".c"compute_list_dispatch" # 4275841770;
    COMPUTE_LIST_END = c"RenderingDevice".c"compute_list_end" # 3218959716;
    BUFFER_GET_DATA = c"RenderingDevice".c"buffer_get_data" # 3101830688;
    FREE_RID = c"RenderingDevice".c"free_rid" # 2722037293;
    TEXTURE_FORMAT_GET_FORMAT = c"RDTextureFormat".c"get_format" # 2235804183;
    SHADER_SOURCE_SET_STAGE_SOURCE = c"RDShaderSource".c"set_stage_source" # 620821314;
    SHADER_SPIRV_GET_STAGE_COMPILE_ERROR = c"RDShaderSPIRV".c"get_stage_compile_error" # 3354920045;
    UNIFORM_SET_UNIFORM_TYPE = c"RDUniform".c"set_uniform_type" # 1664894931;
    UNIFORM_SET_BINDING = c"RDUniform".c"set_binding" # 1286410249;
    UNIFORM_ADD_ID = c"RDUniform".c"add_id" # 2722037293;
}

/// The builtin constructors, destructors and methods, and the engine method binds.
pub(crate) struct Godot {
    pub(crate) interface: &'static Interface,
    pub(crate) library: *mut c_void,
    string_name_destroy: PtrDestructor,
    string_new: PtrConstructor,
    string_destroy: PtrDestructor,
    dictionary_new: PtrConstructor,
    dictionary_copy: PtrConstructor,
    dictionary_destroy: PtrDestructor,
    dictionary_has: PtrBuiltinMethod,
    array_new: PtrConstructor,
    array_destroy: PtrDestructor,
    array_push_back: PtrBuiltinMethod,
    bytes_new: PtrConstructor,
    bytes_destroy: PtrDestructor,
    bytes_size: PtrBuiltinMethod,
    callable_destroy: PtrDestructor,
    pub(crate) from_bool: FromTypeConstructor,
    pub(crate) from_int: FromTypeConstructor,
    pub(crate) from_float: FromTypeConstructor,
    pub(crate) from_string: FromTypeConstructor,
    pub(crate) from_vector2i: FromTypeConstructor,
    pub(crate) from_transform3d: FromTypeConstructor,
    pub(crate) from_projection: FromTypeConstructor,
    pub(crate) from_object: FromTypeConstructor,
    pub(crate) from_dictionary: FromTypeConstructor,
    pub(crate) to_int: ToTypeConstructor,
    pub(crate) to_float: ToTypeConstructor,
    pub(crate) to_rid: ToTypeConstructor,
    to_dictionary: ToTypeConstructor,
    /// Null where the engine has no method of that name and hash.
    binds: Vec<MethodBindPtr>,
}

// SAFETY: the library pointer and method binds are process-lived engine identities, and every
// interface function is callable from any thread Godot calls this extension on.
unsafe impl Send for Godot {}
unsafe impl Sync for Godot {}

static INTERFACE: OnceLock<Interface> = OnceLock::new();
static LIBRARY: OnceLock<usize> = OnceLock::new();
static GODOT: OnceLock<Godot> = OnceLock::new();

/// Publishes the interface at library load. Later loads keep the first one.
pub(crate) fn publish_interface(interface: Interface, library: *mut c_void) -> &'static Interface {
    let _ = LIBRARY.set(library as usize);
    INTERFACE.get_or_init(|| interface)
}

pub(crate) fn interface() -> Option<&'static Interface> {
    INTERFACE.get()
}

/// The builtins and binds once the scene level resolved them.
pub(crate) fn godot() -> Option<&'static Godot> {
    GODOT.get()
}

/// Resolves the builtins and binds on the first scene initialization.
pub(crate) fn resolve() -> Option<&'static Godot> {
    if let Some(godot) = GODOT.get() {
        return Some(godot);
    }
    let interface = INTERFACE.get()?;
    let library = *LIBRARY.get()? as *mut c_void;
    let godot = unsafe { Godot::resolve(interface, library) }?;
    Some(GODOT.get_or_init(|| godot))
}

impl Godot {
    /// # Safety
    /// The interface came from the running engine and its core types are initialized.
    unsafe fn resolve(interface: &'static Interface, library: *mut c_void) -> Option<Self> {
        let i = interface;
        let constructor = |kind, index| unsafe { (i.variant_get_ptr_constructor)(kind, index) };
        let destructor = |kind| unsafe { (i.variant_get_ptr_destructor)(kind) };
        let from = |kind| unsafe { (i.get_variant_from_type_constructor)(kind) };
        let to = |kind| unsafe { (i.get_variant_to_type_constructor)(kind) };
        let string_name_destroy = destructor(TYPE_STRING_NAME)?;
        let builtin = |kind, name: &CStr, hash| {
            let mut method = Opaque::<STRING_NAME_SIZE>::zeroed();
            unsafe {
                (i.string_name_new_with_latin1_chars)(
                    method.0.as_mut_ptr().cast(),
                    name.as_ptr(),
                    0,
                )
            };
            let found =
                unsafe { (i.variant_get_ptr_builtin_method)(kind, method.0.as_ptr().cast(), hash) };
            unsafe { string_name_destroy(method.0.as_mut_ptr().cast()) };
            found
        };
        let mut godot = Self {
            interface,
            library,
            string_name_destroy,
            string_new: constructor(TYPE_STRING, 0)?,
            string_destroy: destructor(TYPE_STRING)?,
            dictionary_new: constructor(TYPE_DICTIONARY, 0)?,
            dictionary_copy: constructor(TYPE_DICTIONARY, 1)?,
            dictionary_destroy: destructor(TYPE_DICTIONARY)?,
            dictionary_has: builtin(TYPE_DICTIONARY, c"has", 3680194679)?,
            array_new: constructor(TYPE_ARRAY, 0)?,
            array_destroy: destructor(TYPE_ARRAY)?,
            array_push_back: builtin(TYPE_ARRAY, c"push_back", 3316032543)?,
            bytes_new: constructor(TYPE_PACKED_BYTE_ARRAY, 0)?,
            bytes_destroy: destructor(TYPE_PACKED_BYTE_ARRAY)?,
            bytes_size: builtin(TYPE_PACKED_BYTE_ARRAY, c"size", 3173160232)?,
            callable_destroy: destructor(TYPE_CALLABLE)?,
            from_bool: from(TYPE_BOOL)?,
            from_int: from(TYPE_INT)?,
            from_float: from(TYPE_FLOAT)?,
            from_string: from(TYPE_STRING)?,
            from_vector2i: from(TYPE_VECTOR2I)?,
            from_transform3d: from(TYPE_TRANSFORM3D)?,
            from_projection: from(TYPE_PROJECTION)?,
            from_object: from(TYPE_OBJECT)?,
            from_dictionary: from(TYPE_DICTIONARY)?,
            to_int: to(TYPE_INT)?,
            to_float: to(TYPE_FLOAT)?,
            to_rid: to(TYPE_RID)?,
            to_dictionary: to(TYPE_DICTIONARY)?,
            binds: Vec::with_capacity(METHODS.len()),
        };
        for &(class, method, hash) in METHODS {
            let class = StringName::new(&godot, class);
            let method = StringName::new(&godot, method);
            let bind = unsafe { (i.classdb_get_method_bind)(class.ptr(), method.ptr(), hash) };
            godot.binds.push(bind);
        }
        Some(godot)
    }

    pub(crate) fn singleton(&self, name: &CStr) -> ObjectPtr {
        let name = StringName::new(self, name);
        unsafe { (self.interface.global_get_singleton)(name.ptr()) }
    }

    pub(crate) fn rendering_server(&self) -> ObjectPtr {
        self.singleton(c"RenderingServer")
    }

    /// A new engine object with the one reference a RefCounted class starts with in 4.7.
    pub(crate) fn construct(&self, class: &CStr) -> ObjectPtr {
        let class = StringName::new(self, class);
        unsafe { (self.interface.classdb_construct_object3)(class.ptr()) }
    }

    /// A new RefCounted engine object, owned by the returned reference.
    pub(crate) fn instantiate(&'static self, class: &CStr) -> Option<Ref> {
        Ref::adopt(self, self.construct(class))
    }

    /// # Safety
    /// `args` match the bind's ptrcall argument encodings; `ret` is the encoded return storage,
    /// or unused for a void method.
    unsafe fn ptrcall(
        &self,
        method: Bind,
        object: ObjectPtr,
        args: &[ConstTypePtr],
        ret: TypePtr,
    ) -> bool {
        let bind = self.binds[method as usize];
        if bind.is_null() || object.is_null() {
            return false;
        }
        unsafe { (self.interface.object_method_bind_ptrcall)(bind, object, args.as_ptr(), ret) };
        true
    }

    /// A void method.
    ///
    /// # Safety
    /// As for `ptrcall`.
    pub(crate) unsafe fn call(
        &self,
        method: Bind,
        object: ObjectPtr,
        args: &[ConstTypePtr],
    ) -> bool {
        unsafe { self.ptrcall(method, object, args, std::ptr::null_mut()) }
    }

    /// A method returning a plain value: `i64` for integers and enums, `u8` for a boolean, `u64`
    /// for a RID, an object pointer, or a single-precision vector, transform or projection.
    ///
    /// # Safety
    /// As for `ptrcall`; `R` is the return's encoding.
    pub(crate) unsafe fn value<R: Copy>(
        &self,
        method: Bind,
        object: ObjectPtr,
        args: &[ConstTypePtr],
        initial: R,
    ) -> Option<R> {
        let mut value = initial;
        unsafe { self.ptrcall(method, object, args, (&raw mut value).cast()) }.then_some(value)
    }

    /// # Safety
    /// As for `ptrcall`; the method returns a `String`.
    pub(crate) unsafe fn string(
        &'static self,
        method: Bind,
        object: ObjectPtr,
        args: &[ConstTypePtr],
    ) -> Option<GString> {
        let mut value = GString::empty(self);
        unsafe { self.ptrcall(method, object, args, value.ptr_mut()) }.then_some(value)
    }

    /// # Safety
    /// As for `ptrcall`; the method returns a `PackedByteArray`.
    pub(crate) unsafe fn bytes(
        &'static self,
        method: Bind,
        object: ObjectPtr,
        args: &[ConstTypePtr],
    ) -> Option<PackedBytes> {
        let mut value = PackedBytes::new(self);
        unsafe { self.ptrcall(method, object, args, value.ptr_mut()) }.then_some(value)
    }

    /// The owned reference a `Ref<T>` method returns; `None` for a null reference.
    ///
    /// # Safety
    /// As for `ptrcall`; the method returns a `Ref<T>`.
    pub(crate) unsafe fn reference(
        &'static self,
        method: Bind,
        object: ObjectPtr,
        args: &[ConstTypePtr],
    ) -> Option<Ref> {
        let returned = unsafe { self.value(method, object, args, std::ptr::null_mut::<c_void>()) }?;
        Ref::adopt(self, returned)
    }

    /// `Object.is_class`, which 4.7 recommends over the deprecated class tag casts.
    pub(crate) fn is_class(&self, object: ObjectPtr, class: &CStr) -> bool {
        let class = StringName::new(self, class);
        let answer = unsafe { self.value(Bind::OBJECT_IS_CLASS, object, &[class.ptr()], 0u8) };
        answer == Some(1)
    }

    /// `Object.notification(NOTIFICATION_POSTINITIALIZE)`, as godot-cpp sends it on request.
    pub(crate) fn postinitialize(&self, object: ObjectPtr) {
        let (what, reversed) = (NOTIFICATION_POSTINITIALIZE, 0u8);
        unsafe {
            self.call(
                Bind::OBJECT_NOTIFICATION,
                object,
                &[arg(&what), arg(&reversed)],
            )
        };
    }
}

/// The ptrcall argument pointer of a value.
pub(crate) fn arg<T>(value: &T) -> ConstTypePtr {
    (value as *const T).cast()
}

/// An owned `StringName`.
pub(crate) struct StringName {
    godot_destroy: PtrDestructor,
    value: Opaque<STRING_NAME_SIZE>,
}

impl StringName {
    pub(crate) fn new(godot: &Godot, name: &CStr) -> Self {
        Self::with(godot.interface, godot.string_name_destroy, name)
    }

    fn with(interface: &Interface, destroy: PtrDestructor, name: &CStr) -> Self {
        let mut value = Opaque::zeroed();
        unsafe {
            (interface.string_name_new_with_latin1_chars)(
                value.0.as_mut_ptr().cast(),
                name.as_ptr(),
                0,
            )
        };
        Self {
            godot_destroy: destroy,
            value,
        }
    }

    pub(crate) fn ptr(&self) -> ConstTypePtr {
        self.value.0.as_ptr().cast()
    }

    pub(crate) fn ptr_mut(&mut self) -> TypePtr {
        self.value.0.as_mut_ptr().cast()
    }

    /// Interned names compare by their data pointer, as `StringName::operator==` does.
    ///
    /// # Safety
    /// `other` points to a live StringName.
    pub(crate) unsafe fn same(&self, other: *const c_void) -> bool {
        !other.is_null()
            && unsafe { other.cast::<[u8; STRING_NAME_SIZE]>().read_unaligned() } == self.value.0
    }
}

impl Drop for StringName {
    fn drop(&mut self) {
        unsafe { (self.godot_destroy)(self.ptr_mut()) };
    }
}

/// An owned `String`.
pub(crate) struct GString {
    godot: &'static Godot,
    value: Opaque<STRING_SIZE>,
}

impl GString {
    pub(crate) fn empty(godot: &'static Godot) -> Self {
        let mut value = Opaque::zeroed();
        unsafe { (godot.string_new)(value.0.as_mut_ptr().cast(), std::ptr::null()) };
        Self { godot, value }
    }

    pub(crate) fn new(godot: &'static Godot, text: &CStr) -> Self {
        let mut value = Opaque::zeroed();
        unsafe {
            (godot.interface.string_new_with_utf8_chars)(value.0.as_mut_ptr().cast(), text.as_ptr())
        };
        Self { godot, value }
    }

    pub(crate) fn ptr(&self) -> ConstTypePtr {
        self.value.0.as_ptr().cast()
    }

    pub(crate) fn ptr_mut(&mut self) -> TypePtr {
        self.value.0.as_mut_ptr().cast()
    }

    pub(crate) fn utf8(&self) -> Vec<u8> {
        let length = unsafe {
            (self.godot.interface.string_to_utf8_chars)(self.ptr(), std::ptr::null_mut(), 0)
        };
        let mut bytes = vec![0u8; usize::try_from(length).unwrap_or(0)];
        if !bytes.is_empty() {
            let written = unsafe {
                (self.godot.interface.string_to_utf8_chars)(
                    self.ptr(),
                    bytes.as_mut_ptr().cast(),
                    length,
                )
            };
            bytes.truncate(usize::try_from(written).unwrap_or(0));
        }
        bytes
    }

    pub(crate) fn is_empty(&self) -> bool {
        let length = unsafe {
            (self.godot.interface.string_to_utf8_chars)(self.ptr(), std::ptr::null_mut(), 0)
        };
        length <= 0
    }
}

impl Drop for GString {
    fn drop(&mut self) {
        unsafe { (self.godot.string_destroy)(self.ptr_mut()) };
    }
}

/// An owned `Variant`.
pub(crate) struct Variant {
    godot: &'static Godot,
    value: Opaque<VARIANT_SIZE>,
}

impl Variant {
    /// # Safety
    /// `from` is the from-type constructor of the type `value` points to.
    pub(crate) unsafe fn from(
        godot: &'static Godot,
        from: FromTypeConstructor,
        value: ConstTypePtr,
    ) -> Self {
        let mut variant = Opaque::zeroed();
        unsafe { from(variant.0.as_mut_ptr().cast(), value.cast_mut()) };
        Self {
            godot,
            value: variant,
        }
    }

    pub(crate) fn string(godot: &'static Godot, text: &CStr) -> Self {
        let text = GString::new(godot, text);
        unsafe { Self::from(godot, godot.from_string, text.ptr()) }
    }

    pub(crate) fn ptr(&self) -> ConstTypePtr {
        self.value.0.as_ptr().cast()
    }
}

impl Drop for Variant {
    fn drop(&mut self) {
        unsafe { (self.godot.interface.variant_destroy)(self.value.0.as_mut_ptr().cast()) };
    }
}

/// Replaces the Variant at `slot` with `value`'s type, as godot-cpp's assignment does.
///
/// # Safety
/// `slot` is a live Variant; `from` matches `value`.
pub(crate) unsafe fn assign(
    godot: &Godot,
    slot: *mut c_void,
    from: FromTypeConstructor,
    value: ConstTypePtr,
) {
    unsafe {
        (godot.interface.variant_destroy)(slot);
        from(slot, value.cast_mut());
    }
}

/// An owned `Dictionary`.
pub(crate) struct Dictionary {
    godot: &'static Godot,
    value: Opaque<DICTIONARY_SIZE>,
}

impl Dictionary {
    pub(crate) fn new(godot: &'static Godot) -> Self {
        let mut value = Opaque::zeroed();
        unsafe { (godot.dictionary_new)(value.0.as_mut_ptr().cast(), std::ptr::null()) };
        Self { godot, value }
    }

    /// The Dictionary a Variant holds, converted by its to-type constructor.
    ///
    /// # Safety
    /// `variant` is a live Variant.
    pub(crate) unsafe fn of_variant(godot: &'static Godot, variant: *const c_void) -> Self {
        let mut value = Opaque::zeroed();
        unsafe { (godot.to_dictionary)(value.0.as_mut_ptr().cast(), variant.cast_mut()) };
        Self { godot, value }
    }

    pub(crate) fn ptr(&self) -> ConstTypePtr {
        self.value.0.as_ptr().cast()
    }

    pub(crate) fn ptr_mut(&mut self) -> TypePtr {
        self.value.0.as_mut_ptr().cast()
    }

    /// Assigns this Dictionary to the constructed Dictionary at `target`.
    ///
    /// # Safety
    /// `target` is a live Dictionary.
    pub(crate) unsafe fn assign_to(&self, target: TypePtr) {
        let args = [self.ptr()];
        unsafe {
            (self.godot.dictionary_destroy)(target);
            (self.godot.dictionary_copy)(target, args.as_ptr());
        }
    }
}

impl Drop for Dictionary {
    fn drop(&mut self) {
        unsafe { (self.godot.dictionary_destroy)(self.ptr_mut()) };
    }
}

/// `Dictionary::has` and the const `operator[]` on a borrowed Dictionary.
///
/// # Safety
/// `dictionary` is a live Dictionary.
pub(crate) unsafe fn dictionary_get(
    godot: &Godot,
    dictionary: ConstTypePtr,
    key: &Variant,
) -> Option<*mut c_void> {
    let mut has = 0u8;
    let args = [key.ptr()];
    unsafe {
        (godot.dictionary_has)(
            dictionary.cast_mut(),
            args.as_ptr(),
            (&raw mut has).cast(),
            1,
        )
    };
    if has == 0 {
        return None;
    }
    let value = unsafe { (godot.interface.dictionary_operator_index_const)(dictionary, key.ptr()) };
    (!value.is_null()).then_some(value)
}

/// The inserting `operator[]` slot of `key`.
///
/// # Safety
/// `dictionary` is a live Dictionary.
pub(crate) unsafe fn dictionary_slot(
    godot: &Godot,
    dictionary: TypePtr,
    key: &Variant,
) -> *mut c_void {
    unsafe { (godot.interface.dictionary_operator_index)(dictionary, key.ptr()) }
}

/// An owned `Array`, typed to an object class as godot-cpp's `TypedArray<T>` is.
pub(crate) struct TypedArray {
    godot: &'static Godot,
    value: Opaque<ARRAY_SIZE>,
}

impl TypedArray {
    pub(crate) fn new(godot: &'static Godot, class: &CStr) -> Self {
        let mut value = Opaque::zeroed();
        unsafe { (godot.array_new)(value.0.as_mut_ptr().cast(), std::ptr::null()) };
        let mut array = Self { godot, value };
        let class = StringName::new(godot, class);
        let script = Opaque::<VARIANT_SIZE>::zeroed();
        unsafe {
            (godot.interface.array_set_typed)(
                array.ptr_mut(),
                TYPE_OBJECT,
                class.ptr(),
                script.0.as_ptr().cast(),
            )
        };
        array
    }

    pub(crate) fn ptr(&self) -> ConstTypePtr {
        self.value.0.as_ptr().cast()
    }

    fn ptr_mut(&mut self) -> TypePtr {
        self.value.0.as_mut_ptr().cast()
    }

    pub(crate) fn push_object(&mut self, object: ObjectPtr) {
        let variant = unsafe { Variant::from(self.godot, self.godot.from_object, arg(&object)) };
        let args = [variant.ptr()];
        unsafe {
            (self.godot.array_push_back)(self.ptr_mut(), args.as_ptr(), std::ptr::null_mut(), 1)
        };
    }
}

impl Drop for TypedArray {
    fn drop(&mut self) {
        unsafe { (self.godot.array_destroy)(self.ptr_mut()) };
    }
}

/// An owned `PackedByteArray`.
pub(crate) struct PackedBytes {
    godot: &'static Godot,
    value: Opaque<PACKED_BYTE_ARRAY_SIZE>,
}

impl PackedBytes {
    pub(crate) fn new(godot: &'static Godot) -> Self {
        let mut value = Opaque::zeroed();
        unsafe { (godot.bytes_new)(value.0.as_mut_ptr().cast(), std::ptr::null()) };
        Self { godot, value }
    }

    pub(crate) fn ptr(&self) -> ConstTypePtr {
        self.value.0.as_ptr().cast()
    }

    fn ptr_mut(&mut self) -> TypePtr {
        self.value.0.as_mut_ptr().cast()
    }

    /// The bytes and their count; null and zero when empty.
    pub(crate) fn data(&self) -> (*const u8, u64) {
        let mut size = 0i64;
        unsafe {
            (self.godot.bytes_size)(
                self.ptr().cast_mut(),
                std::ptr::null(),
                (&raw mut size).cast(),
                0,
            )
        };
        if size <= 0 {
            return (std::ptr::null(), 0);
        }
        let data =
            unsafe { (self.godot.interface.packed_byte_array_operator_index_const)(self.ptr(), 0) };
        if data.is_null() {
            (data, 0)
        } else {
            (data, size as u64)
        }
    }
}

impl Drop for PackedBytes {
    fn drop(&mut self) {
        unsafe { (self.godot.bytes_destroy)(self.ptr_mut()) };
    }
}

/// An owned custom `Callable` that calls `call` with no arguments.
pub(crate) struct Callable {
    godot: &'static Godot,
    value: Opaque<CALLABLE_SIZE>,
}

impl Callable {
    pub(crate) fn custom(
        godot: &'static Godot,
        call: CallableCall,
        count: CallableArgumentCount,
    ) -> Self {
        let mut info = CallableCustomInfo2 {
            callable_userdata: std::ptr::null_mut(),
            token: godot.library,
            object_id: 0,
            call_func: Some(call),
            unused_callbacks: [std::ptr::null(); 6],
            get_argument_count_func: Some(count),
        };
        let mut value = Opaque::zeroed();
        unsafe {
            (godot.interface.callable_custom_create2)(value.0.as_mut_ptr().cast(), &mut info)
        };
        Self { godot, value }
    }

    pub(crate) fn ptr(&self) -> ConstTypePtr {
        self.value.0.as_ptr().cast()
    }
}

impl Drop for Callable {
    fn drop(&mut self) {
        unsafe { (self.godot.callable_destroy)(self.value.0.as_mut_ptr().cast()) };
    }
}

/// One owned reference to a RefCounted engine object, released as godot-cpp's `Ref<T>` does.
pub(crate) struct Ref {
    godot: &'static Godot,
    object: ObjectPtr,
}

impl Ref {
    /// Takes ownership of one reference already counted for `object`.
    pub(crate) fn adopt(godot: &'static Godot, object: ObjectPtr) -> Option<Self> {
        (!object.is_null()).then_some(Self { godot, object })
    }

    pub(crate) fn object(&self) -> ObjectPtr {
        self.object
    }
}

impl Drop for Ref {
    fn drop(&mut self) {
        let last = unsafe {
            self.godot
                .value(Bind::REF_COUNTED_UNREFERENCE, self.object, &[], 0u8)
        };
        if last == Some(1) {
            unsafe { (self.godot.interface.object_destroy)(self.object) };
        }
    }
}

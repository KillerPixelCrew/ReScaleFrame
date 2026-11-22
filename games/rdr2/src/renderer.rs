// SPDX-License-Identifier: MIT
//! Native DLSS and FSR2 SR input routing. Both selections normalize to the game's DLSS
//! direct-resource descriptor and lend the same leased SR pass to the host. Any refusal returns to
//! the game's own upscaler for that frame and resets RSF history.
use crate::camera::{self, CameraInputs, CameraResult};
use crate::host::{Host, HostCell};
use crate::layout::{
    EVALUATE, Evaluation, FRAME_TIME, FSR_IMPORT, FSR_WRAPPER_RETURN, IMAGE_SIZE, IMAGE_TIMESTAMP,
    NGX_CREATE_FLAGS, NGX_DEPTH_INVERTED, NGX_FAIL, NGX_FAIL_MASK, NGX_GET_INT_SLOT,
    NGX_MV_JITTERED, NGX_SUCCESS, POP, SUBMIT_RETURN, SWAPCHAIN, block, d3d, fsr, fsr_dispatch,
    view,
};
use crate::platform::{
    self, ImageFacts, Platform, ResourceFacts, Scope, image_facts, lock, trampoline,
};
use crate::scene;
use crate::{BUSY, NATIVE_REFUSED, NOT_READY, OK};
use rsf_game_sys::{
    RSF_GAME_RENDER_FEEDBACK_ACCEPTED, RSF_GAME_RENDER_FEEDBACK_CAMERA_JITTER_INVALID,
    RSF_GAME_RENDER_FEEDBACK_CAMERA_UNAVAILABLE, RSF_GAME_RENDER_FEEDBACK_HOST_REFUSED,
    RSF_GAME_RENDER_FEEDBACK_INPUT_UNSUPPORTED, RSF_GAME_RENDER_FEEDBACK_OUTPUT_UNSUPPORTED,
    RSF_GAME_RENDER_FEEDBACK_PRESENT_UNAVAILABLE, RSF_GAME_RENDER_FEEDBACK_SUBRECT_UNSUPPORTED,
    RSF_GAME_RENDER_NO_FG, RSF_GAME_RENDER_PRIMARY, RSF_GAME_RENDER_RESET, RSF_GAME_RENDER_SR,
    rsf_game_host_services, rsf_game_render_pass, rsf_game_renderer_status,
};
use rsf_game_sys::{RSF_SCREEN_FLIGHT, RSF_SCREEN_UNKNOWN};
use std::collections::BTreeMap;
use std::ffi::{CStr, c_char, c_void};
use std::ptr;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicU64, AtomicUsize, Ordering};

type EvaluateFn =
    unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, *const Evaluation) -> u32;
type PopFn = unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void;
type ExecuteFn = unsafe extern "system" fn(*mut c_void, u32, *const *mut c_void);
type PresentFn = unsafe extern "system" fn(*mut c_void, u32, u32) -> i32;
type FsrDispatchFn = unsafe extern "C" fn(*mut c_void, *const c_void) -> i32;
type FsrCreateFn = unsafe extern "C" fn(*mut c_void, *const c_void) -> i32;
type NgxGetIntFn = unsafe extern "C" fn(*mut c_void, *const c_char, *mut i32) -> u32;

/// Returned only if a detour runs without its trampoline, which MinHook never allows.
const NO_TRAMPOLINE: i32 = i32::MIN;
const FSR_CREATE_REPORTS: u32 = 4;

static PREPARED: AtomicBool = AtomicBool::new(false);
static ACTIVE: AtomicBool = AtomicBool::new(false);
static CALLBACKS: AtomicU32 = AtomicU32::new(0);
static NEXT_SUBMISSION: AtomicU64 = AtomicU64::new(0);
static IMAGE: AtomicUsize = AtomicUsize::new(0);
static HOST: HostCell = HostCell::new();
static ORIGINAL_EVALUATE: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static ORIGINAL_POP: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static ORIGINAL_FSR: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static ORIGINAL_FSR_CREATE: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
/// ffxFsr2ContextDispatch inside the game's own ffx_fsr2_api_x64.dll. Detouring the export reaches
/// every caller; the earlier import-slot patch at +6032a98 saw no call.
static FSR_EXPORT: AtomicUsize = AtomicUsize::new(0);
static FSR_CREATE_EXPORT: AtomicUsize = AtomicUsize::new(0);
static FSR_CALLER: AtomicUsize = AtomicUsize::new(0);
static FSR_CREATES: AtomicU32 = AtomicU32::new(0);
static REFUSAL: AtomicU32 = AtomicU32::new(u32::MAX);

/// The game's DIRECT queue and actual swapchain. Each holds one reference and points at a cloned
/// table whose entries call into this module, so a live observer pins the plugin; process teardown
/// owns them until an explicit queue-drain/unhook protocol exists.
struct Observer {
    active: usize,
    observed: bool,
    originals: BTreeMap<usize, usize>,
}

impl Observer {
    const fn new() -> Self {
        Self {
            active: 0,
            observed: false,
            originals: BTreeMap::new(),
        }
    }

    fn select(&mut self, candidate: usize, attach: impl FnOnce(*mut *mut c_void) -> bool) -> bool {
        let previous = self.active;
        let mut original = ptr::null_mut();
        self.active = if candidate == 0 {
            0
        } else {
            match self.originals.entry(candidate) {
                std::collections::btree_map::Entry::Occupied(_) => candidate,
                std::collections::btree_map::Entry::Vacant(slot) => {
                    if attach(&mut original) {
                        slot.insert(original as usize);
                        candidate
                    } else {
                        0
                    }
                }
            }
        };
        previous != self.active
    }
}

struct Observers {
    queue: Observer,
    swapchain: Observer,
    generation: u32,
}

impl Observers {
    fn changed(&mut self) {
        self.generation = self.generation.wrapping_add(1).max(1);
        self.queue.observed = false;
        self.swapchain.observed = false;
        scene::set_swapchain(self.swapchain.active, self.generation);
    }

    fn ready(&self) -> bool {
        self.queue.observed && self.swapchain.observed
    }
}

static OBSERVERS: Mutex<Observers> = Mutex::new(Observers {
    queue: Observer::new(),
    swapchain: Observer::new(),
    generation: 0,
});

#[derive(Clone, Copy)]
pub(crate) struct History {
    pub(crate) frame: u64,
    pub(crate) jitter: [f32; 2],
    generation: u32,
    pub(crate) native_gap: bool,
    pub(crate) native_result: u32,
}

impl Default for History {
    fn default() -> Self {
        Self {
            frame: 0,
            jitter: [0.0; 2],
            generation: 0,
            native_gap: false,
            native_result: u32::MAX,
        }
    }
}

/// Upscale workers change each frame. This serializes a feature's history and the shared host
/// callback; it never surrounds queue submission or a GPU wait on unsubmitted work.
static EVALUATIONS: Mutex<BTreeMap<usize, History>> = Mutex::new(BTreeMap::new());
static FSR_CONTEXTS: Mutex<BTreeMap<usize, FsrContext>> = Mutex::new(BTreeMap::new());

#[derive(Default)]
struct FsrContext {
    flags: Option<u32>,
    history: History,
}

impl History {
    fn replaced(&mut self) {
        self.native_gap = true;
    }

    fn fell_back(&mut self, result: u32, recovered: bool) -> bool {
        let report = self.native_gap || self.native_result != result;
        self.native_result = result;
        self.native_gap &= !recovered;
        self.frame = 0;
        report
    }
}

fn fsr_camera_flags(flags: u32) -> Option<u32> {
    if flags & fsr::INVERTED_DEPTH == 0
        || flags & (fsr::DISPLAY_MOTION | fsr::INFINITE_DEPTH | !fsr::KNOWN_FLAGS) != 0
    {
        return None;
    }
    let motion = if flags & fsr::JITTERED_MOTION != 0 {
        NGX_MV_JITTERED
    } else {
        0
    };
    Some(NGX_DEPTH_INVERTED | motion)
}

#[repr(align(8))]
struct FsrDescription([u8; fsr::FULL_DESCRIPTION_BYTES]);

impl FsrDescription {
    fn fallback(
        &mut self,
        original: *const c_void,
        force_reset: bool,
        dispatch: impl FnOnce(*const c_void) -> i32,
    ) -> i32 {
        if force_reset {
            self.0[fsr::RESET] = 1;
            dispatch(self.0.as_ptr().cast())
        } else {
            dispatch(original)
        }
    }
}

struct Context<'a> {
    platform: &'a Platform,
    host: Host,
    image: usize,
}

struct Request {
    list: *mut c_void,
    feature: usize,
    flags: u32,
    /// FSR supplies the engine's resource states; DLSS uses the measured defaults.
    engine_states: Option<[u32; 5]>,
}

fn refusal(host: &Host, reason: u32, message: &str) -> bool {
    host.feedback(reason, 0);
    if REFUSAL.swap(reason, Ordering::SeqCst) != reason {
        host.log(message);
    }
    false
}

/// Measured engine states for color, depth, motion, output and exposure at the DLSS evaluation.
const DLSS_STATES: [u32; 5] = [
    d3d::PIXEL_SHADER_RESOURCE | d3d::NON_PIXEL_SHADER_RESOURCE,
    d3d::DEPTH_READ,
    d3d::PIXEL_SHADER_RESOURCE | d3d::NON_PIXEL_SHADER_RESOURCE,
    d3d::UNORDERED_ACCESS,
    d3d::UNORDERED_ACCESS,
];
/// States the host receives: shader-readable inputs and a copy-destination output.
const HOST_STATES: [u32; 5] = [
    d3d::NON_PIXEL_SHADER_RESOURCE,
    d3d::NON_PIXEL_SHADER_RESOURCE,
    d3d::NON_PIXEL_SHADER_RESOURCE,
    d3d::COPY_DEST,
    d3d::NON_PIXEL_SHADER_RESOURCE,
];

fn transition(
    platform: &Platform,
    list: *mut c_void,
    resources: &[*mut c_void; 5],
    before: &[u32; 5],
    after: &[u32; 5],
) {
    for ((&resource, &from), &to) in resources.iter().zip(before).zip(after) {
        if !resource.is_null() && from != to {
            unsafe { (platform.barrier)(list, resource, from, to) };
        }
    }
}

fn unsupported(request: &Request, d: &Evaluation) -> bool {
    request.list.is_null()
        || request.feature == 0
        || d.color.is_null()
        || d.output.is_null()
        || d.depth.is_null()
        || d.motion.is_null()
        || d.width == 0
        || d.height == 0
        || d.invert_x != 0
        || d.invert_y != 0
        || (d.pre_exposure != 0.0 && d.pre_exposure != 1.0)
}

/// The worker view, native frame and frame time, when the NGX feature was created depth-inverted.
fn sample(ctx: &Context, flags: u32, camera: &mut [u8; view::BYTES]) -> Option<(usize, u64, f32)> {
    let (view_address, frame) = scene::render_block(ctx.platform, ctx.image, block::VIEW)?;
    if !ctx.platform.read_bytes(view_address, camera) {
        return None;
    }
    let dt = ctx
        .platform
        .read_value::<f32>(ctx.image.wrapping_add(FRAME_TIME))?;
    let valid = dt.is_finite() && dt > 0.0 && dt <= 1.0 && flags & NGX_DEPTH_INVERTED != 0;
    valid.then_some((view_address, frame, dt))
}

fn process(ctx: &Context, request: &Request, d: &Evaluation, history: &mut History) -> bool {
    let host = &ctx.host;
    let (Some(process_upscale), true) = (host.0.process_upscale, host.0.submit.is_some()) else {
        return refusal(
            host,
            RSF_GAME_RENDER_FEEDBACK_INPUT_UNSUPPORTED,
            "RDR2 SR native fallback: unsupported evaluation descriptor or inactive host.",
        );
    };
    if !ACTIVE.load(Ordering::SeqCst) || unsupported(request, d) {
        return refusal(
            host,
            RSF_GAME_RENDER_FEEDBACK_INPUT_UNSUPPORTED,
            "RDR2 SR native fallback: unsupported evaluation descriptor or inactive host.",
        );
    }
    if d.origins.iter().any(|&origin| origin != 0) {
        return refusal(
            host,
            RSF_GAME_RENDER_FEEDBACK_SUBRECT_UNSUPPORTED,
            "RDR2 SR native fallback: nonzero input/output subrect origin.",
        );
    }
    let active_swapchain = ctx.platform.read_pointer(ctx.image.wrapping_add(SWAPCHAIN));
    let generation = {
        let observers = lock(&OBSERVERS);
        (active_swapchain == Some(observers.swapchain.active) && observers.ready())
            .then_some(observers.generation)
    };
    let Some(generation) = generation else {
        return refusal(
            host,
            RSF_GAME_RENDER_FEEDBACK_PRESENT_UNAVAILABLE,
            "RDR2 SR native fallback: current game queue/Present pair has not been observed.",
        );
    };
    let mut camera = [0u8; view::BYTES];
    let Some((view_address, frame, dt)) = sample(ctx, request.flags, &mut camera) else {
        return refusal(
            host,
            RSF_GAME_RENDER_FEEDBACK_CAMERA_UNAVAILABLE,
            "RDR2 SR native fallback: worker camera, frame time or NGX creation flags unavailable.",
        );
    };
    let mut output = ResourceFacts::default();
    if !unsafe { (ctx.platform.resource_facts)(d.output, &mut output) }
        || output.width > d3d::MAX_EXTENT
        || u64::from(output.height) > d3d::MAX_EXTENT
        || output.format != d3d::R16G16B16A16_FLOAT
    {
        return refusal(
            host,
            RSF_GAME_RENDER_FEEDBACK_OUTPUT_UNSUPPORTED,
            "RDR2 SR native fallback: output format/extent unsupported.",
        );
    }
    let reset =
        d.reset != 0 || history.generation != generation || history.frame.wrapping_add(1) != frame;
    let input = CameraInputs {
        render_width: d.width,
        render_height: d.height,
        output_width: output.width as u32,
        output_height: output.height,
        jitter: d.jitter,
        previous_jitter: history.jitter,
        frame_time_seconds: dt,
        reset,
        motion_jittered: request.flags & NGX_MV_JITTERED != 0,
    };
    let mut decoded = CameraResult::default();
    if !camera::decode(&camera, &input, &mut decoded) || !decoded.projection_jitter_matches {
        return refusal(
            host,
            RSF_GAME_RENDER_FEEDBACK_CAMERA_JITTER_INVALID,
            "RDR2 SR native fallback: render projection is invalid or its jitter is frozen.",
        );
    }
    // Source identity comes from observed native input/simulation/render markers. Actual Present
    // still requires the later scene/resource match on the submission thread.
    let source = scene::source(frame);
    let mut pass = sr_pass(
        host,
        request,
        d,
        &output,
        &decoded,
        (view_address, frame, reset, source),
    );
    pass.resource_generation = generation;
    let resources = [d.color, d.depth, d.motion, d.output, d.exposure];
    let before = request.engine_states.unwrap_or(DLSS_STATES);
    transition(
        ctx.platform,
        request.list,
        &resources,
        &before,
        &HOST_STATES,
    );
    let accepted = unsafe { process_upscale(host.0.user, request.list, &pass) } == 0;
    transition(
        ctx.platform,
        request.list,
        &resources,
        &HOST_STATES,
        &before,
    );
    history.frame = frame;
    history.generation = generation;
    history.jitter = d.jitter;
    host.feedback(
        if accepted {
            RSF_GAME_RENDER_FEEDBACK_ACCEPTED
        } else {
            RSF_GAME_RENDER_FEEDBACK_HOST_REFUSED
        },
        frame,
    );
    if accepted {
        scene::upscale(request.list as usize, &pass);
    }
    accepted
}

/// `identity` is (worker view, native frame, reset, source frame).
fn sr_pass(
    host: &Host,
    request: &Request,
    d: &Evaluation,
    output: &ResourceFacts,
    decoded: &CameraResult,
    identity: (usize, u64, bool, u64),
) -> rsf_game_render_pass {
    let (view_address, frame, reset, source) = identity;
    // Null resources, zero keys and no depth domains.
    let mut pass: rsf_game_render_pass = unsafe { std::mem::zeroed() };
    pass.struct_size = size_of::<rsf_game_render_pass>() as u32;
    pass.role = RSF_GAME_RENDER_SR;
    pass.session_id = host.session();
    let key = request.feature as u64;
    pass.family_key = key;
    pass.view_key = key;
    pass.history_key = key;
    pass.pass_key = frame;
    pass.native_frame = frame;
    pass.scope_id = view_address as u64;
    pass.resource_generation = 1;
    pass.flags = RSF_GAME_RENDER_PRIMARY | if reset { RSF_GAME_RENDER_RESET } else { 0 };
    pass.render_rect[2] = d.width as i32;
    pass.render_rect[3] = d.height as i32;
    pass.output_rect[2] = output.width as i32;
    pass.output_rect[3] = output.height as i32;
    pass.color_input = d.color;
    pass.color_output = d.output;
    pass.depth = d.depth;
    pass.motion = d.motion;
    pass.exposure = d.exposure;
    pass.camera = decoded.camera;
    pass.camera_valid = 1;
    pass.source_frame_id = source;
    pass.screen = if source != 0 {
        RSF_SCREEN_FLIGHT
    } else {
        RSF_SCREEN_UNKNOWN
    };
    if source == 0 {
        pass.flags |= RSF_GAME_RENDER_NO_FG;
    }
    pass.previous_clip_to_clip = decoded.previous_clip_to_clip;
    pass.motion_to_uv = [
        -d.motion_scale[0] / d.width as f32,
        -d.motion_scale[1] / d.height as f32,
    ];
    pass.motion_camera_included = 1;
    pass
}

/// The installed NGX parameter interface's `GetI` for the feature's creation flags.
fn feature_flags(platform: &Platform, parameters: *mut c_void) -> Option<u32> {
    let table = platform.read_pointer(parameters as usize)?;
    let entry = platform.read_pointer(table.wrapping_add(NGX_GET_INT_SLOT * size_of::<usize>()))?;
    if entry == 0 {
        return None;
    }
    // The slot holds the interface's GetI(this, name, out) entry.
    let get = unsafe { std::mem::transmute::<usize, NgxGetIntFn>(entry) };
    let mut value = 0i32;
    let result = unsafe { get(parameters, NGX_CREATE_FLAGS.as_ptr(), &mut value) };
    (result == NGX_SUCCESS).then_some(value as u32)
}

fn context(platform: &'static Platform) -> Option<Context<'static>> {
    Some(Context {
        platform,
        host: HOST.get()?,
        image: IMAGE.load(Ordering::SeqCst),
    })
}

unsafe extern "C" fn evaluate(
    list: *mut c_void,
    feature: *mut c_void,
    parameters: *mut c_void,
    pointer: *const Evaluation,
) -> u32 {
    let _callback = Scope::enter(&CALLBACKS);
    let Some(native) = (unsafe { trampoline::<EvaluateFn>(&ORIGINAL_EVALUATE) }) else {
        return NGX_FAIL;
    };
    let ctx = platform::get().and_then(context);
    let mut descriptor = Evaluation::default();
    let copied = ctx.as_ref().is_some_and(|ctx| {
        !pointer.is_null()
            && unsafe {
                (ctx.platform.read)(
                    pointer as usize,
                    (&raw mut descriptor).cast(),
                    size_of::<Evaluation>(),
                )
            }
    });
    let Some(ctx) = ctx.filter(|_| copied) else {
        return unsafe { native(list, feature, parameters, pointer) };
    };
    let mut histories = lock(&EVALUATIONS);
    let history = histories.entry(feature as usize).or_default();
    let request = feature_flags(ctx.platform, parameters).map(|flags| Request {
        list,
        feature: feature as usize,
        flags,
        engine_states: None,
    });
    if let Some(request) = request
        && process(&ctx, &request, &descriptor, history)
    {
        history.replaced();
        // NVSDK_NGX_Result_Success; the outer wrapper still restores RAGE's heap cache.
        return NGX_SUCCESS;
    }
    if history.native_gap {
        descriptor.reset = 1;
    }
    let result = unsafe { native(list, feature, parameters, &descriptor) };
    if history.fell_back(result, result & NGX_FAIL_MASK != NGX_FAIL) {
        ctx.host.log(&format!(
            "RDR2 native DLSS fallback result=0x{result:08x} feature={:016X} reset={}.",
            feature as usize, descriptor.reset
        ));
    }
    result
}

/// Logs each distinct caller module of the private FSR2 export once. The game's wrapper call
/// (+26f050a) need not be the immediate caller: another hook on the import slot forwards from its
/// own module.
fn report_fsr_caller(ctx: &Context, caller: usize) {
    let report = match FSR_CALLER.compare_exchange(0, caller, Ordering::SeqCst, Ordering::SeqCst) {
        Ok(_) => true,
        Err(first) => {
            first != caller && FSR_CALLER.swap(usize::MAX, Ordering::SeqCst) != usize::MAX
        }
    };
    if !report {
        return;
    }
    let mut base = 0usize;
    let mut path = [0u8; 1024];
    unsafe { (ctx.platform.module_path)(caller, &mut base, path.as_mut_ptr().cast(), path.len()) };
    let path = CStr::from_bytes_until_nul(&path)
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default();
    let name = path.rsplit('\\').next().unwrap_or_default();
    let name = if name.is_empty() { "<unknown>" } else { name };
    let measured =
        base != 0 && base == ctx.image && caller.wrapping_sub(ctx.image) == FSR_WRAPPER_RETURN;
    ctx.host.log(&format!(
        "RDR2 native FSR2 dispatch observed from {name}+{:x}{}.",
        caller.wrapping_sub(base),
        if measured { " (measured wrapper)" } else { "" }
    ));
}

/// Entered from the native leaf with the call's return address.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_rdr2_fsr_dispatch_detour(
    context_pointer: *mut c_void,
    pointer: *const c_void,
    caller: usize,
) -> i32 {
    let _callback = Scope::enter(&CALLBACKS);
    let Some(native) = (unsafe { trampoline::<FsrDispatchFn>(&ORIGINAL_FSR) }) else {
        return NO_TRAMPOLINE;
    };
    let Some(ctx) = platform::get().and_then(context) else {
        return unsafe { native(context_pointer, pointer) };
    };
    report_fsr_caller(&ctx, caller);
    let mut contexts = lock(&FSR_CONTEXTS);
    let Some(feature) = contexts.get_mut(&(context_pointer as usize)) else {
        return unsafe { native(context_pointer, pointer) };
    };
    let mut copy = FsrDescription([0; fsr::FULL_DESCRIPTION_BYTES]);
    let copied = ctx.platform.read_bytes(pointer as usize, &mut copy.0);
    if copied && let Some(flags) = feature.flags {
        let prefix = copy.0[..fsr::DESCRIPTION_BYTES]
            .try_into()
            .expect("FSR prefix size");
        let dispatch = fsr_dispatch(prefix);
        let request = Request {
            list: dispatch.command_list,
            feature: context_pointer as usize,
            flags,
            engine_states: Some(dispatch.states),
        };
        if dispatch.states_valid
            && process(&ctx, &request, &dispatch.descriptor, &mut feature.history)
        {
            feature.history.replaced();
            return fsr::FFX_OK;
        }
    }
    let forced_reset = copied && feature.history.native_gap;
    let result = copy.fallback(pointer, forced_reset, |description| unsafe {
        native(context_pointer, description)
    });
    let reset = copied && copy.0[fsr::RESET] != 0;
    if feature
        .history
        .fell_back(result as u32, result == fsr::FFX_OK && reset)
    {
        ctx.host.log(&format!(
            "RDR2 native FSR fallback result={result} context={:016X} reset={}.",
            context_pointer as usize,
            u32::from(reset)
        ));
    }
    result
}

/// FfxFsr2ContextDescription flags, maxRenderSize and displaySize.
unsafe extern "C" fn fsr_create(context_pointer: *mut c_void, description: *const c_void) -> i32 {
    let _callback = Scope::enter(&CALLBACKS);
    let key = context_pointer as usize;
    lock(&FSR_CONTEXTS).remove(&key);
    let fields =
        platform::get().and_then(|platform| platform.read_value::<[u32; 5]>(description as usize));
    let result = match unsafe { trampoline::<FsrCreateFn>(&ORIGINAL_FSR_CREATE) } {
        Some(native) => unsafe { native(context_pointer, description) },
        None => NO_TRAMPOLINE,
    };
    if let Some(fields) = fields {
        if key != 0 && result == fsr::FFX_OK {
            lock(&FSR_CONTEXTS).insert(
                key,
                FsrContext {
                    flags: fsr_camera_flags(fields[0]),
                    history: History::default(),
                },
            );
        }
        if FSR_CREATES.fetch_add(1, Ordering::SeqCst) < FSR_CREATE_REPORTS
            && let Some(host) = HOST.get()
        {
            host.log(&format!(
                "RDR2 native FSR2 context create result={result} flags=0x{:x} max_render={}x{} display={}x{}.",
                fields[0], fields[1], fields[2], fields[3], fields[4]
            ));
        }
    }
    result
}

unsafe extern "system" fn execute(owner: *mut c_void, count: u32, lists: *const *mut c_void) {
    let _callback = Scope::enter(&CALLBACKS);
    let (original, generation, active) = {
        let observers = lock(&OBSERVERS);
        (
            observers.queue.originals.get(&(owner as usize)).copied(),
            observers.generation,
            observers.queue.active == owner as usize,
        )
    };
    let Some(original) = original else { return };
    let native = unsafe { std::mem::transmute::<usize, ExecuteFn>(original) };
    let id = NEXT_SUBMISSION
        .fetch_add(1, Ordering::SeqCst)
        .wrapping_add(1);
    let submit = HOST
        .get()
        .and_then(|host| host.0.submit.map(|submit| (host.0.user, submit)));
    if let Some((user, submit)) = submit {
        unsafe { submit(user, owner, id, count, lists, 1) };
    }
    unsafe { native(owner, count, lists) };
    if let Some((user, submit)) = submit {
        unsafe { submit(user, owner, id, count, lists, 0) };
    }
    if active && count != 0 && !lists.is_null() {
        let mut observers = lock(&OBSERVERS);
        if observers.generation == generation {
            observers.queue.observed = true;
        }
    }
}

unsafe extern "system" fn present(owner: *mut c_void, interval: u32, flags: u32) -> i32 {
    let _callback = Scope::enter(&CALLBACKS);
    let (original, generation, active) = {
        let observers = lock(&OBSERVERS);
        (
            observers
                .swapchain
                .originals
                .get(&(owner as usize))
                .copied(),
            observers.generation,
            observers.swapchain.active == owner as usize,
        )
    };
    let Some(original) = original else {
        return NO_TRAMPOLINE;
    };
    let native = unsafe { std::mem::transmute::<usize, PresentFn>(original) };
    let report = HOST
        .get()
        .filter(|_| active)
        .and_then(|host| host.0.present.map(|present| (host.0.user, present)));
    if let Some((user, present)) = report {
        unsafe { present(user, owner, interval, flags, 1) };
    }
    let receipt = platform::get()
        .filter(|_| active)
        .and_then(|platform| scene::before_present(platform, owner, flags, generation));
    let result = unsafe { native(owner, interval, flags) };
    let observed = {
        let mut observers = lock(&OBSERVERS);
        let observed = observers.generation == generation && receipt.is_some() && result == 0;
        observers.swapchain.observed |= observed;
        observed
    };
    if active {
        scene::after_present(receipt.filter(|_| observed), result);
    }
    if let Some((user, present)) = report {
        unsafe { present(user, owner, interval, flags, 0) };
    }
    result
}

fn select_observers(platform: &Platform, host: &Host, queue: usize, swapchain: usize) {
    let mut observers = lock(&OBSERVERS);
    let queue_changed = observers.queue.select(queue, |original| unsafe {
        (platform.attach_queue)(queue as *mut c_void, execute as *const c_void, original)
    });
    let swapchain_changed = observers.swapchain.select(swapchain, |original| unsafe {
        (platform.attach_present)(swapchain as *mut c_void, present as *const c_void, original)
    });
    if queue_changed || swapchain_changed {
        observers.changed();
        let generation = observers.generation;
        let queue = observers.queue.active;
        let swapchain = observers.swapchain.active;
        drop(observers);
        host.log(&format!("RDR2 queue/Present observer pair changed: generation={generation} queue={queue:016X} swapchain={swapchain:016X}."));
    }
}

/// Entered from the native leaf with the call's return address.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_rdr2_pop_detour(
    ring: *mut c_void,
    record: *mut c_void,
    caller: usize,
) -> *mut c_void {
    let _callback = Scope::enter(&CALLBACKS);
    let image = IMAGE.load(Ordering::SeqCst);
    let game_submit = caller == image.wrapping_add(SUBMIT_RETURN);
    let result = match unsafe { trampoline::<PopFn>(&ORIGINAL_POP) } {
        Some(native) => unsafe { native(ring, record) },
        None => ptr::null_mut(),
    };
    if game_submit
        && ACTIVE.load(Ordering::SeqCst)
        && let Some(ctx) = platform::get().and_then(context)
    {
        let queue = ctx.platform.read_pointer(record as usize).unwrap_or(0);
        let swapchain = ctx
            .platform
            .read_pointer(image.wrapping_add(SWAPCHAIN))
            .unwrap_or(0);
        select_observers(ctx.platform, &ctx.host, queue, swapchain);
    }
    result
}

/// The FSR2 dispatch export, or the reason the route is refused. A refusal leaves native FSR2
/// untouched; the route is independent of the DLSS route.
fn find_fsr_export(platform: &Platform, host: &Host, image: usize) -> Result<usize, &'static str> {
    let module = unsafe { (platform.module_handle)(fsr::MODULE.as_ptr()) };
    if module == 0 {
        return Err("ffx_fsr2_api_x64.dll is not loaded");
    }
    let expected = ImageFacts {
        timestamp: fsr::TIMESTAMP,
        image_size: fsr::IMAGE_SIZE,
    };
    if image_facts(platform, module) != Some(expected) {
        return Err(
            "ffx_fsr2_api_x64.dll is not the 1491.50 build (timestamp 64907ede, image 0x10000)",
        );
    }
    let entry = unsafe { (platform.export_address)(module, fsr::DISPATCH.as_ptr()) };
    if entry != module.wrapping_add(fsr::DISPATCH_RVA)
        || !platform.matches(entry, &fsr::DISPATCH_BYTES)
    {
        return Err("ffxFsr2ContextDispatch is not at +3c70 with the expected entry bytes");
    }
    let create = unsafe { (platform.export_address)(module, fsr::CREATE.as_ptr()) };
    if create == module.wrapping_add(fsr::CREATE_RVA)
        && platform.matches(create, &fsr::CREATE_BYTES)
        && platform.create(create, fsr_create as *const c_void, &ORIGINAL_FSR_CREATE)
    {
        FSR_CREATE_EXPORT.store(create, Ordering::SeqCst);
    }
    let resolves = platform.read_pointer(image.wrapping_add(FSR_IMPORT)) == Some(entry);
    host.log(&format!(
        "RDR2 FSR2 import slot +6032a98 {} ffx_fsr2_api_x64.dll!ffxFsr2ContextDispatch.",
        if resolves {
            "resolves to"
        } else {
            "does NOT resolve to"
        }
    ));
    Ok(entry)
}

pub(crate) fn prepare(services: &rsf_game_host_services) -> i32 {
    if PREPARED.load(Ordering::SeqCst) {
        return BUSY;
    }
    if services.process_upscale.is_none() || services.submit.is_none() || services.present.is_none()
    {
        return NOT_READY;
    }
    let Some(platform) = platform::get() else {
        return NOT_READY;
    };
    let image = unsafe { (platform.main_module)() };
    IMAGE.store(image, Ordering::SeqCst);
    let expected = ImageFacts {
        timestamp: IMAGE_TIMESTAMP,
        image_size: IMAGE_SIZE,
    };
    if image_facts(platform, image) != Some(expected) {
        return NOT_READY;
    }
    if !platform.matches(image.wrapping_add(EVALUATE.rva), &EVALUATE.expected)
        || !platform.matches(image.wrapping_add(POP.rva), &POP.expected)
    {
        return NATIVE_REFUSED;
    }
    if !unsafe { (platform.hook_initialize)() } {
        return NATIVE_REFUSED;
    }
    let host = Host(*services);
    let evaluate_site = image.wrapping_add(EVALUATE.rva);
    if !platform.create(evaluate_site, evaluate as *const c_void, &ORIGINAL_EVALUATE) {
        return NATIVE_REFUSED;
    }
    if !platform.create(
        image.wrapping_add(POP.rva),
        platform.pop_detour,
        &ORIGINAL_POP,
    ) {
        platform.remove(evaluate_site);
        return NATIVE_REFUSED;
    }
    HOST.set(Some(*services));
    PREPARED.store(true, Ordering::SeqCst);
    if !scene::prepare(platform, image, services) {
        host.log("RDR2 completed-scene producer unavailable; SR remains available.");
    }
    let refused = match find_fsr_export(platform, &host, image) {
        Ok(entry) if platform.create(entry, platform.fsr_dispatch_detour, &ORIGINAL_FSR) => {
            FSR_EXPORT.store(entry, Ordering::SeqCst);
            None
        }
        Ok(_) => Some("MinHook could not create the export detour"),
        Err(reason) => Some(reason),
    };
    if let Some(reason) = refused {
        host.log(&format!(
            "RDR2 FSR2 input route refused: {reason}. Native FSR2 is retained."
        ));
    }
    host.log("RDR2 SR hooks prepared: expected bytes match +26f3a3c and +26c97d4.");
    OK
}

pub(crate) fn start() -> i32 {
    let Some(platform) = platform::get() else {
        return NOT_READY;
    };
    if !PREPARED.load(Ordering::SeqCst) {
        return NOT_READY;
    }
    let image = IMAGE.load(Ordering::SeqCst);
    let pop_site = image.wrapping_add(POP.rva);
    if !platform.enable(pop_site) {
        return NATIVE_REFUSED;
    }
    ACTIVE.store(true, Ordering::SeqCst);
    if !platform.enable(image.wrapping_add(EVALUATE.rva)) {
        ACTIVE.store(false, Ordering::SeqCst);
        platform.disable(pop_site);
        return NATIVE_REFUSED;
    }
    let host = HOST.get();
    let log = |message: &str| {
        if let Some(host) = host {
            host.log(message);
        }
    };
    let fsr_export = FSR_EXPORT.load(Ordering::SeqCst);
    let fsr_create_export = FSR_CREATE_EXPORT.load(Ordering::SeqCst);
    let fsr = fsr_export != 0
        && fsr_create_export != 0
        && platform.enable(fsr_create_export)
        && platform.enable(fsr_export);
    if fsr_export != 0 && !fsr {
        log(
            "RDR2 FSR2 input route refused: export detour could not be enabled. Native FSR2 is retained.",
        );
    }
    if !scene::start() {
        log("RDR2 completed-scene producer did not activate.");
    }
    log(if fsr {
        "RDR2 DLSS and FSR2 (ffxFsr2ContextDispatch export) selections route to the common SR adapter."
    } else {
        "RDR2 DLSS selection routes to the common SR adapter; native FSR2 is not intercepted."
    });
    OK
}

pub(crate) fn quiesce() -> i32 {
    ACTIVE.store(false, Ordering::SeqCst);
    // The scene producer is quiesced only once no renderer callback is running.
    if CALLBACKS.load(Ordering::SeqCst) != 0 || !scene::quiesce() {
        BUSY
    } else {
        OK
    }
}

pub(crate) fn stop() -> i32 {
    if ACTIVE.load(Ordering::SeqCst) || CALLBACKS.load(Ordering::SeqCst) != 0 {
        return BUSY;
    }
    if !PREPARED.load(Ordering::SeqCst) {
        return OK;
    }
    let Some(platform) = platform::get() else {
        return BUSY;
    };
    {
        // A live observer table pins the module; process teardown owns it.
        let observers = lock(&OBSERVERS);
        if !observers.queue.originals.is_empty() || !observers.swapchain.originals.is_empty() {
            return BUSY;
        }
    }
    if !scene::stop() {
        return BUSY;
    }
    let image = IMAGE.load(Ordering::SeqCst);
    let fsr_export = FSR_EXPORT.load(Ordering::SeqCst);
    let fsr_create_export = FSR_CREATE_EXPORT.load(Ordering::SeqCst);
    let mut sites = [
        fsr_export,
        fsr_create_export,
        image.wrapping_add(EVALUATE.rva),
        image.wrapping_add(POP.rva),
    ]
    .into_iter()
    .filter(|&site| site != 0);
    if !sites.clone().all(|site| platform.disable(site))
        || CALLBACKS.load(Ordering::SeqCst) != 0
        || !sites.all(|site| platform.remove(site))
    {
        return BUSY;
    }
    FSR_EXPORT.store(0, Ordering::SeqCst);
    FSR_CREATE_EXPORT.store(0, Ordering::SeqCst);
    lock(&FSR_CONTEXTS).clear();
    lock(&EVALUATIONS).clear();
    PREPARED.store(false, Ordering::SeqCst);
    HOST.set(None);
    OK
}

pub(crate) fn status(status: &mut rsf_game_renderer_status) {
    status.prepared = u32::from(PREPARED.load(Ordering::SeqCst));
    status.active = u32::from(ACTIVE.load(Ordering::SeqCst));
    status.rendering_ready = 0;
    status.reason = c"Native DLSS SR replacement implemented; FSR2 export routing, live acceptance and FG remain pending."
        .as_ptr();
}

#[cfg(test)]
mod tests;

// SPDX-License-Identifier: MIT
//! Independent fixture model ported from the Anomaly temporal C++ headers.
//! It uses column-vector math and does not call the live SR, motion, or camera owners.
use super::*;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

pub(super) mod math;

#[derive(Clone, Default, PartialEq)]
pub(super) struct Pose {
    pub world: Matrix,
    pub palette_layout: u64,
    pub deformation_layout: u64,
    pub bones: Vec<Matrix>,
    pub constants: Vec<Vec4>,
    pub fade: f64,
    pub domain: u8,
    pub kind: u8,
    pub has_deformation: bool,
    pub has_detail: bool,
}

impl Pose {
    fn valid(&self) -> bool {
        if !math::affine(&self.world) || !matches!(self.domain, WORLD | FIRST_PERSON) {
            return false;
        }
        if self.kind == SKINNED {
            if self.palette_layout == 0
                || !(1..=1024).contains(&self.bones.len())
                || !self.bones.iter().all(math::affine)
            {
                return false;
            }
        } else if !self.bones.is_empty() || self.palette_layout != 0 {
            return false;
        }
        if self.has_deformation
            && (self.deformation_layout == 0
                || !(1..=256).contains(&self.constants.len())
                || !self.constants.iter().copied().all(math::finite_point))
        {
            return false;
        }
        match self.kind {
            0 => !self.has_deformation && !self.has_detail,
            SKINNED => !self.has_detail,
            DEFORMED => self.has_deformation && !self.has_detail,
            DETAIL => {
                self.has_deformation && self.has_detail && self.fade.is_finite() && self.fade >= 0.
            }
            _ => false,
        }
    }

    fn compatible(&self, other: &Self) -> bool {
        (
            self.domain,
            self.kind,
            self.palette_layout,
            self.bones.len(),
        ) == (
            other.domain,
            other.kind,
            other.palette_layout,
            other.bones.len(),
        ) && self.has_deformation == other.has_deformation
            && (!self.has_deformation
                || (self.deformation_layout, self.constants.len())
                    == (other.deformation_layout, other.constants.len()))
    }

    fn frozen(&self) -> Self {
        let mut pose = self.clone();
        if !pose.has_deformation {
            pose.deformation_layout = 0;
            pose.constants.clear();
        }
        if !pose.has_detail {
            pose.fade = 0.;
        }
        pose
    }
}

pub(super) struct Sample {
    pub world: Matrix,
    pub bone1: Matrix,
    pub deformation_w: f64,
    pub fade: f64,
}
impl From<&Pose> for Sample {
    fn from(pose: &Pose) -> Self {
        Self {
            world: pose.world,
            bone1: pose.bones.get(1).copied().unwrap_or_default(),
            deformation_w: pose.constants.first().map_or(0., |constant| constant.w),
            fade: pose.fade,
        }
    }
}

pub(super) struct Capture {
    status: u32,
    current: Option<Rc<Pose>>,
    previous: Option<Rc<Pose>>,
}
impl Capture {
    fn refused(status: u32) -> Self {
        Self {
            status,
            current: None,
            previous: None,
        }
    }
    pub fn status(&self) -> u32 {
        self.status
    }
    pub fn current(&self) -> Option<Sample> {
        self.current.as_deref().map(Sample::from)
    }
    pub fn previous(&self) -> Option<Sample> {
        self.previous.as_deref().map(Sample::from)
    }
    pub fn handles(&self) -> (Option<*const Pose>, Option<*const Pose>) {
        (
            self.current.as_ref().map(Rc::as_ptr),
            self.previous.as_ref().map(Rc::as_ptr),
        )
    }
}

pub(super) struct Pair {
    pub current: Camera,
    pub previous: Camera,
    pub previous_frame: u64,
    pub has_previous: bool,
}

pub(super) struct Plan {
    pub token: Token,
    pub sizes: Sizes,
    pub screen_ui: Extent,
    pub world: Matrix,
    pub first_person: Matrix,
    pub has_first_person: bool,
    pub presentation: u32,
}

type Instances = BTreeMap<(Key, u8), Rc<Pose>>;

#[derive(Default)]
struct View {
    current: Option<Frame>,
    previous: Option<Frame>,
    current_instances: Instances,
    previous_instances: Instances,
    serial: u64,
}

pub(super) struct History {
    owner: u64,
    serial: u64,
    views: [View; 2],
    retired: BTreeMap<(u64, u64), u64>,
}

fn valid_key(key: Key) -> bool {
    key.owner != 0 && key.visual != 0 && key.generation != 0
}

fn frame_camera(frame: &Frame, domain: u8) -> Option<Camera> {
    match domain {
        WORLD => Some(frame.world),
        FIRST_PERSON if frame.has_first_person => Some(frame.first_person),
        _ => None,
    }
}

fn raster(camera: Camera, render: Extent) -> Option<Matrix> {
    math::jitter(&camera.projection, camera.jitter.x, camera.jitter.y, render)
}

fn previous_camera(view: &View, domain: u8) -> Option<(Camera, u64)> {
    let current = view.current?;
    let previous = view.previous?;
    if domain == FIRST_PERSON && current.first_person_cut {
        return None;
    }
    Some((frame_camera(&previous, domain)?, previous.source_frame))
}

impl History {
    pub fn new() -> Self {
        static NEXT_OWNER: AtomicU64 = AtomicU64::new(1);
        let owner = NEXT_OWNER
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |next| {
                next.checked_add(1)
            })
            .unwrap_or(0);
        Self {
            owner,
            serial: 0,
            views: Default::default(),
            retired: BTreeMap::new(),
        }
    }

    pub fn begin(&mut self, frame: &Frame) -> (u32, Token) {
        let invalid = (INVALID, Token::default());
        let Some(view) = self.views.get_mut(usize::from(frame.view)) else {
            return invalid;
        };
        let render = frame.sizes.render;
        let valid_camera = |camera: Camera| {
            math::affine(&camera.view)
                && math::inverse(&camera.view).is_some()
                && math::inverse(&camera.projection).is_some()
                && raster(camera, render).is_some()
        };
        if self.owner == 0
            || math::resolutions(&frame.sizes).is_none()
            || !valid_camera(frame.world)
            || (frame.has_first_person && !valid_camera(frame.first_person))
        {
            return invalid;
        }
        let mut frame = *frame;
        if !frame.has_first_person {
            frame.first_person = Camera::default();
        }
        let make_token = |serial| Token {
            view: frame.view,
            source_frame: frame.source_frame,
            serial,
            owner: self.owner,
        };
        if let Some(current) = view.current {
            match frame.source_frame.cmp(&current.source_frame) {
                std::cmp::Ordering::Less => return (OUT_OF_ORDER, Token::default()),
                std::cmp::Ordering::Equal if current == frame => {
                    return (REPEATED, make_token(view.serial));
                }
                std::cmp::Ordering::Equal => return (FRAME_CONFLICT, Token::default()),
                std::cmp::Ordering::Greater => {}
            }
        }
        let Some(serial) = self.serial.checked_add(1) else {
            return invalid;
        };
        let reset = frame.camera_cut
            || view
                .current
                .is_some_and(|current| current.sizes != frame.sizes);
        view.previous = if reset { None } else { view.current };
        view.previous_instances = if reset {
            BTreeMap::new()
        } else {
            std::mem::take(&mut view.current_instances)
        };
        view.current_instances.clear();
        view.current = Some(frame);
        view.serial = serial;
        self.serial = serial;
        (STARTED, make_token(serial))
    }

    pub fn start(&mut self, frame: &Frame) -> Token {
        let (status, token) = self.begin(frame);
        assert_eq!(status, STARTED);
        assert_ne!(token.owner, 0);
        token
    }

    fn live_index(&self, token: Token) -> Option<usize> {
        let index = usize::from(token.view);
        let view = self.views.get(index)?;
        (token.owner == self.owner
            && view.serial == token.serial
            && view.current?.source_frame == token.source_frame)
            .then_some(index)
    }

    pub fn cameras(&self, token: Token, domain: u8) -> Option<Pair> {
        let view = &self.views[self.live_index(token)?];
        let current = frame_camera(&view.current?, domain)?;
        let previous = previous_camera(view, domain);
        let (camera, frame) = previous.unwrap_or_default();
        Some(Pair {
            current,
            previous: camera,
            previous_frame: frame,
            has_previous: previous.is_some(),
        })
    }

    pub fn plan(&self, token: Token) -> Option<Plan> {
        let frame = self.views[self.live_index(token)?].current?;
        let world = raster(frame.world, frame.sizes.render)?;
        let first_person = if frame.has_first_person {
            raster(frame.first_person, frame.sizes.render)?
        } else {
            Matrix::default()
        };
        Some(Plan {
            token,
            sizes: frame.sizes,
            screen_ui: frame.sizes.output,
            world,
            first_person,
            has_first_person: frame.has_first_person,
            presentation: u32::from(frame.view == SCOPE),
        })
    }

    pub fn capture(&mut self, token: Token, key: Key, pose: &Pose) -> Capture {
        if !valid_key(key) || !pose.valid() {
            return Capture::refused(CAPTURE_INVALID);
        }
        if self
            .retired
            .get(&(key.owner, key.visual))
            .is_some_and(|&generation| key.generation <= generation)
        {
            return Capture::refused(RETIRED);
        }
        let Some(index) = self.live_index(token) else {
            return Capture::refused(STALE);
        };
        let view = &mut self.views[index];
        if view
            .current
            .as_ref()
            .and_then(|frame| frame_camera(frame, pose.domain))
            .is_none()
        {
            return Capture::refused(MISSING_PROJECTION);
        }
        let slot = (key, pose.domain);
        let previous = previous_camera(view, pose.domain)
            .and_then(|_| view.previous_instances.get(&slot))
            .filter(|old| old.compatible(pose))
            .cloned();
        let frozen = pose.frozen();
        if let Some(current) = view.current_instances.get(&slot) {
            return if **current == frozen {
                Capture {
                    status: CAPTURE_REPEATED,
                    current: Some(current.clone()),
                    previous,
                }
            } else {
                Capture::refused(CAPTURE_CONFLICT)
            };
        }
        let current = Rc::new(frozen);
        view.current_instances.insert(slot, current.clone());
        Capture {
            status: CAPTURED,
            current: Some(current),
            previous,
        }
    }

    pub fn retire(&mut self, key: Key) -> bool {
        if !valid_key(key) {
            return false;
        }
        let watermark = self.retired.entry((key.owner, key.visual)).or_default();
        *watermark = (*watermark).max(key.generation);
        let generation = *watermark;
        for view in &mut self.views {
            let keep = |(entry, _): &(Key, u8), _: &mut Rc<Pose>| {
                (entry.owner, entry.visual) != (key.owner, key.visual)
                    || entry.generation > generation
            };
            view.current_instances.retain(keep);
            view.previous_instances.retain(keep);
        }
        true
    }

    pub fn reset_view(&mut self, view: u8) -> bool {
        let Some(view) = self.views.get_mut(usize::from(view)) else {
            return false;
        };
        *view = View::default();
        true
    }

    pub fn reset_all(&mut self) {
        self.views = Default::default();
    }
}

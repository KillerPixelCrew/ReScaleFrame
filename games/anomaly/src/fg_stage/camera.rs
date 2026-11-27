// SPDX-License-Identifier: MIT
use crate::camera::{self, EngineMatrix};
use rsf_game_sys::rsf_camera_frame;

#[derive(Clone, Copy)]
pub(super) struct Camera {
    view: EngineMatrix,
    projection: EngineMatrix,
}

impl Camera {
    pub(super) fn new(view: EngineMatrix, projection: EngineMatrix) -> Option<Self> {
        view.iter()
            .chain(&projection)
            .all(|value| value.is_finite())
            .then_some(Self { view, projection })
    }

    pub(super) fn is_cut_from(self, previous: Self) -> bool {
        camera::camera_cut(
            camera::pose(&previous.view),
            camera::pose(&self.view),
            4.0,
            45.0,
        )
    }

    pub(super) fn frame(
        self,
        previous: Self,
        [width, height]: [u32; 2],
        frame_time_seconds: f32,
    ) -> Option<(rsf_camera_frame, EngineMatrix)> {
        camera::frame(
            &self.view,
            &self.projection,
            &previous.view,
            &previous.projection,
            camera::projection_planes(&self.projection)?,
            [width, height, width, height],
            frame_time_seconds,
        )
    }
}

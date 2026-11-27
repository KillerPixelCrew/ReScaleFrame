// SPDX-License-Identifier: MIT
use super::*;

use oracle::math::{inverse, jitter, motion, multiply, read, resolutions, transform, write};

pub(super) fn independent_dimensions() {
    let plan = resolutions(&sizes()).unwrap();
    assert_eq!(
        [plan.output, plan.render, plan.pda].map(dimension),
        [(1920, 1080), (1280, 720), (1024, 512)]
    );
}

pub(super) fn dimension_validation() {
    for [ow, oh, rw, rh, pw, ph] in [
        [0, 1080, 640, 360, 512, 512],
        [1920, 1080, 1, 360, 512, 512],
        [1920, 1080, 640, 360, 512, 1],
        [1920, 1080, 2048, 360, 512, 512],
        [20000, 1080, 640, 360, 512, 512],
    ] {
        assert!(
            resolutions(&Sizes {
                output: extent(ow, oh),
                render: extent(rw, rh),
                pda: extent(pw, ph)
            })
            .is_none()
        );
    }
    let minimum = extent(2, 2);
    assert!(
        resolutions(&Sizes {
            output: minimum,
            render: minimum,
            pda: minimum
        })
        .is_some()
    );
}

pub(super) fn matrix_motion() {
    let mut world = identity();
    world.values[3] = 0.2;
    let mvp = multiply(&identity(), &world).unwrap();
    let p = [0., 0., 0.5, 1.];
    let current = transform(&mvp, p).unwrap();
    let previous = transform(&identity(), p).unwrap();
    let size = extent(100, 50);
    let x = motion(current, previous, size, [0.; 2]).unwrap();
    assert!((x[0] + 10.).abs() < 1e-9 && x[1] == 0.);
    let y = motion([0., 0.4, 0.5, 1.], previous, size, [0.; 2]).unwrap();
    assert!((y[1] - 10.).abs() < 1e-9);
    let size = extent(128, 64);
    let reference = jitter(&identity(), 0.25, -0.5, size).unwrap();
    let production = identity().values.map(|x| x as f32);
    let actual = crate::sr_swap::jitter_projection(&production, 0.25, -0.5, 128, 64).unwrap();
    let to_column = |i| f64::from(actual[(i % 4) * 4 + i / 4]);
    let actual = Matrix {
        values: std::array::from_fn(to_column),
    };
    assert_eq!(actual.values, reference.values);
    assert!(crate::sr_swap::jitter_projection(&production, 0.25, -0.5, 1, 64).is_none());
    let clip = transform(&actual, p).unwrap();
    let stable = motion(clip, p, size, [0.25, -0.5]).unwrap();
    assert!(stable.into_iter().all(|x| x.abs() < 1e-9));
}

pub(super) fn inverse_and_projective() {
    let mut view = identity();
    view.values[3] = -2.;
    let mut projection = Matrix::default();
    for (i, v) in [(0, 2.), (5, 3.), (10, 1.1), (11, -0.11), (14, 1.)] {
        projection.values[i] = v;
    }
    let vp = multiply(&projection, &view).unwrap();
    let clip = transform(&vp, [2., 0., 1., 1.]).unwrap();
    assert_eq!((clip[0], clip[3]), (0., 1.));
    let recovered = transform(&inverse(&vp).unwrap(), clip).unwrap();
    assert!((recovered[0] - 2.).abs() < 1e-9 && (recovered[2] - 1.).abs() < 1e-9);
    assert!(inverse(&Matrix::default()).is_none());
    let p = [0., 0., 0.5, 1.];
    let size = extent(100, 50);
    let raster = jitter(&projection, 0.5, 0.25, size).unwrap();
    let raster_clip = transform(&raster, p).unwrap();
    let plain = transform(&projection, p).unwrap();
    let stable = motion(raster_clip, plain, size, [0.5, 0.25]).unwrap();
    assert!(stable.into_iter().all(|x| x.abs() < 1e-9));
    let mut invalid = identity();
    invalid.values[0] = f64::NAN;
    assert!(
        inverse(&invalid).is_none()
            && multiply(&invalid, &view).is_none()
            && transform(&invalid, [0., 0., 1., 1.]).is_none()
    );
    invalid.values[0] = f64::MAX;
    assert!(
        multiply(&invalid, &invalid).is_none() && transform(&invalid, [2., 0., 1., 1.]).is_none()
    );
    for clip in [[0.; 4], [0., 0., 0.5, -1.], [0., 0., 2., 1.]] {
        assert!(motion(clip, p, size, [0.; 2]).is_none());
    }
    assert!(motion(plain, plain, size, [f64::INFINITY, 0.]).is_none());
}

pub(super) fn matrix_indices() {
    let mut matrix = identity();
    assert!(!write(&mut matrix, 0, 4, 99.));
    assert_eq!(read(&matrix, 1, 0), Some(0.));
    assert!(read(&matrix, usize::MAX, 4).is_none());
}

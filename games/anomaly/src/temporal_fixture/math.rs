// SPDX-License-Identifier: MIT
use super::*;

fn resolutions(api: &Oracle, input: &Sizes) -> Option<Sizes> {
    let mut out = Sizes::default();
    unsafe { api.resolutions.unwrap()(input, &mut out) }.then_some(out)
}
pub(super) fn jitter(api: &Oracle, p: &Matrix, x: f64, y: f64, size: Extent) -> Option<Matrix> {
    let mut out = Matrix::default();
    unsafe { api.jitter.unwrap()(p, Vec2 { x, y }, size, &mut out) }.then_some(out)
}
fn multiply(api: &Oracle, a: &Matrix, b: &Matrix) -> Option<Matrix> {
    let mut out = Matrix::default();
    unsafe { api.multiply.unwrap()(a, b, &mut out) }.then_some(out)
}
fn inverse(api: &Oracle, matrix: &Matrix) -> Option<Matrix> {
    let mut out = Matrix::default();
    unsafe { api.inverse.unwrap()(matrix, &mut out) }.then_some(out)
}
fn transform(api: &Oracle, m: &Matrix, [x, y, z, w]: [f64; 4]) -> Option<[f64; 4]> {
    let mut out = Vec4::default();
    unsafe { api.transform.unwrap()(m, Vec4 { x, y, z, w }, &mut out) }
        .then_some([out.x, out.y, out.z, out.w])
}
fn motion(
    api: &Oracle,
    current: [f64; 4],
    previous: [f64; 4],
    size: Extent,
    [x, y]: [f64; 2],
) -> Option<[f64; 2]> {
    let point = |[x, y, z, w]| Vec4 { x, y, z, w };
    let mut out = Vec2::default();
    unsafe {
        api.pixel_motion.unwrap()(
            point(current),
            point(previous),
            size,
            Vec2 { x, y },
            Vec2::default(),
            &mut out,
        )
    }
    .then_some([out.x, out.y])
}

pub(super) fn independent_dimensions(api: &Oracle) {
    let plan = resolutions(api, &sizes()).unwrap();
    assert_eq!(
        [plan.output, plan.render, plan.pda].map(dimension),
        [(1920, 1080), (1280, 720), (1024, 512)]
    );
}

pub(super) fn dimension_validation(api: &Oracle) {
    for [ow, oh, rw, rh, pw, ph] in [
        [0, 1080, 640, 360, 512, 512],
        [1920, 1080, 1, 360, 512, 512],
        [1920, 1080, 640, 360, 512, 1],
        [1920, 1080, 2048, 360, 512, 512],
        [20000, 1080, 640, 360, 512, 512],
    ] {
        assert!(
            resolutions(
                api,
                &Sizes {
                    output: extent(ow, oh),
                    render: extent(rw, rh),
                    pda: extent(pw, ph)
                }
            )
            .is_none()
        );
    }
    let minimum = extent(2, 2);
    assert!(
        resolutions(
            api,
            &Sizes {
                output: minimum,
                render: minimum,
                pda: minimum
            }
        )
        .is_some()
    );
}

pub(super) fn matrix_motion(api: &Oracle) {
    let mut world = identity();
    world.values[3] = 0.2;
    let mvp = multiply(api, &identity(), &world).unwrap();
    let p = [0., 0., 0.5, 1.];
    let current = transform(api, &mvp, p).unwrap();
    let previous = transform(api, &identity(), p).unwrap();
    let size = extent(100, 50);
    let x = motion(api, current, previous, size, [0.; 2]).unwrap();
    assert!((x[0] + 10.).abs() < 1e-9 && x[1] == 0.);
    let y = motion(api, [0., 0.4, 0.5, 1.], previous, size, [0.; 2]).unwrap();
    assert!((y[1] - 10.).abs() < 1e-9);
    let size = extent(128, 64);
    let reference = jitter(api, &identity(), 0.25, -0.5, size).unwrap();
    let production = identity().values.map(|x| x as f32);
    let actual = crate::sr_swap::jitter_projection(&production, 0.25, -0.5, 128, 64).unwrap();
    let to_column = |i| f64::from(actual[(i % 4) * 4 + i / 4]);
    let actual = Matrix {
        values: std::array::from_fn(to_column),
    };
    assert_eq!(actual.values, reference.values);
    assert!(crate::sr_swap::jitter_projection(&production, 0.25, -0.5, 1, 64).is_none());
    let clip = transform(api, &actual, p).unwrap();
    let stable = motion(api, clip, p, size, [0.25, -0.5]).unwrap();
    assert!(stable.into_iter().all(|x| x.abs() < 1e-9));
}

pub(super) fn inverse_and_projective(api: &Oracle) {
    let mut view = identity();
    view.values[3] = -2.;
    let mut projection = Matrix::default();
    for (i, v) in [(0, 2.), (5, 3.), (10, 1.1), (11, -0.11), (14, 1.)] {
        projection.values[i] = v;
    }
    let vp = multiply(api, &projection, &view).unwrap();
    let clip = transform(api, &vp, [2., 0., 1., 1.]).unwrap();
    assert_eq!((clip[0], clip[3]), (0., 1.));
    let recovered = transform(api, &inverse(api, &vp).unwrap(), clip).unwrap();
    assert!((recovered[0] - 2.).abs() < 1e-9 && (recovered[2] - 1.).abs() < 1e-9);
    assert!(inverse(api, &Matrix::default()).is_none());
    let p = [0., 0., 0.5, 1.];
    let size = extent(100, 50);
    let raster = jitter(api, &projection, 0.5, 0.25, size).unwrap();
    let raster_clip = transform(api, &raster, p).unwrap();
    let plain = transform(api, &projection, p).unwrap();
    let stable = motion(api, raster_clip, plain, size, [0.5, 0.25]).unwrap();
    assert!(stable.into_iter().all(|x| x.abs() < 1e-9));
    let mut invalid = identity();
    invalid.values[0] = f64::NAN;
    assert!(
        inverse(api, &invalid).is_none()
            && multiply(api, &invalid, &view).is_none()
            && transform(api, &invalid, [0., 0., 1., 1.]).is_none()
    );
    invalid.values[0] = f64::MAX;
    assert!(
        multiply(api, &invalid, &invalid).is_none()
            && transform(api, &invalid, [2., 0., 1., 1.]).is_none()
    );
    for clip in [[0.; 4], [0., 0., 0.5, -1.], [0., 0., 2., 1.]] {
        assert!(motion(api, clip, p, size, [0.; 2]).is_none());
    }
    assert!(motion(api, plain, plain, size, [f64::INFINITY, 0.]).is_none());
}

pub(super) fn matrix_indices(api: &Oracle) {
    let mut matrix = identity();
    assert!(!unsafe { api.matrix_write.unwrap()(&mut matrix, 0, 4, 99.) });
    let mut value = f64::NAN;
    assert!(unsafe { api.matrix_read.unwrap()(&matrix, 1, 0, &mut value) });
    assert_eq!(value, 0.);
    assert!(!unsafe { api.matrix_read.unwrap()(&matrix, usize::MAX, 4, &mut value) });
}

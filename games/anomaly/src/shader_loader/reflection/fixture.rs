// SPDX-License-Identifier: MIT
use super::*;

#[derive(Clone)]
struct TestBuffer {
    name: &'static [u8],
    size: u32,
    variables: Vec<(&'static [u8], u32, u32)>,
}

#[derive(Clone, Default)]
pub(in crate::shader_loader) struct TestReflection {
    version: u32,
    inputs: Vec<Parameter>,
    outputs: Vec<Parameter>,
    patch: Vec<Parameter>,
    bindings: Vec<Binding>,
    buffers: Vec<TestBuffer>,
}

impl TestReflection {
    pub(in crate::shader_loader) fn snapshot(&self) -> Snapshot {
        let inputs = self.inputs.clone();
        let outputs = self.outputs.clone();
        let patch = self.patch.clone();
        let bindings = self.bindings.clone();
        let variables: Vec<Vec<_>> = self
            .buffers
            .iter()
            .map(|buffer| {
                buffer
                    .variables
                    .iter()
                    .map(|&(name, offset, size)| Variable {
                        name: Bytes::from(name),
                        offset,
                        size,
                    })
                    .collect()
            })
            .collect();
        let buffers: Vec<_> = self
            .buffers
            .iter()
            .zip(&variables)
            .map(|(buffer, rows)| Buffer {
                name: Bytes::from(buffer.name),
                size: buffer.size,
                variables: Span::from(rows.as_slice()),
            })
            .collect();
        Snapshot {
            view: Reflection {
                version: self.version,
                inputs: Span::from(inputs.as_slice()),
                outputs: Span::from(outputs.as_slice()),
                patch: Span::from(patch.as_slice()),
                bindings: Span::from(bindings.as_slice()),
                buffers: Span::from(buffers.as_slice()),
            },
            _inputs: inputs,
            _outputs: outputs,
            _patch: patch,
            _bindings: bindings,
            _buffers: buffers,
            _variables: variables,
            _names: Vec::new(),
        }
    }

    pub(in crate::shader_loader) fn break_buffer(&mut self) {
        self.buffers[0].size = 80;
    }
}

fn param(
    name: &'static [u8],
    index: u32,
    register: u32,
    mask: u32,
    system_value: u32,
) -> Parameter {
    Parameter {
        name: Bytes::from(name),
        index,
        register,
        system_value,
        component: 3,
        mask,
    }
}

fn tex(index: u32, register: u32, mask: u32) -> Parameter {
    param(b"TEXCOORD", index, register, mask, 0)
}

fn position(register: u32, mask: u32) -> Parameter {
    param(b"SV_Position", 0, register, mask, 1)
}

fn target(index: u32) -> Parameter {
    param(b"SV_Target", index, index, 15, 64)
}

fn binding(name: &'static [u8], kind: u32, point: u32) -> Binding {
    Binding {
        name: Bytes::from(name),
        kind,
        point,
        count: 1,
        return_type: 0,
        dimension: 0,
    }
}

fn core_buffer(stage: u8) -> &'static [u8] {
    if stage == b'v' {
        b"RSFCorePrevious"
    } else {
        b"RSFCorePixel"
    }
}

pub(in crate::shader_loader) fn core(stage: u8, variant: bool) -> TestReflection {
    let native_link = vec![tex(0, 0, 3), position(1, 15)];
    let mut r = TestReflection {
        version: if stage == b'v' { 0x10050 } else { 0x50 },
        bindings: vec![
            binding(b"s_base", 3, 0),
            binding(b"$Globals", 0, 0),
            binding(b"dynamic", 0, 1),
        ],
        buffers: vec![
            TestBuffer {
                name: b"$Globals",
                size: 64,
                variables: vec![(b"m_WVP", 0, 64)],
            },
            TestBuffer {
                name: b"dynamic",
                size: 32,
                variables: vec![(b"L_hemi", 0, 16)],
            },
        ],
        ..Default::default()
    };
    if stage == b'v' {
        let mut input = param(b"POSITION", 0, 0, 15, 0);
        input.component = 0;
        r.inputs = vec![input, param(b"NORMAL", 0, 1, 15, 0)];
        r.outputs = native_link;
    } else {
        r.inputs = native_link;
        r.outputs = vec![target(0), target(1)];
    }
    if variant {
        r.bindings.push(binding(core_buffer(stage), 0, 13));
        let patched_link = vec![tex(12, 0, 15), tex(13, 1, 1), tex(0, 2, 3), position(3, 15)];
        if stage == b'v' {
            r.outputs = patched_link;
        } else {
            r.inputs = patched_link;
            r.outputs.push(target(2));
        }
    }
    r
}

fn compatible(stage: u8, stock: &TestReflection, variant: &TestReflection) -> Compatibility {
    rsf_anomaly_shader_compatible(stage, stock.snapshot().view(), variant.snapshot().view())
}

fn linkable(vs: &TestReflection, ps: &TestReflection) -> bool {
    rsf_anomaly_shader_linkable(vs.snapshot().view(), ps.snapshot().view())
}

pub(in crate::shader_loader) fn compatibility() {
    use Compatibility::*;
    for stage in [b'v', b'p'] {
        let stock = core(stage, false);
        let variant = core(stage, true);
        let check = |expected, edit: fn(&mut TestReflection)| {
            let mut edited = variant.clone();
            edit(&mut edited);
            assert_eq!(compatible(stage, &stock, &edited).name(), expected);
        };
        assert!(compatible(stage, &stock, &variant) == Ok);
        check(MissingBinding.name(), |r| r.bindings[0].point = 1);
        check(ExtraBinding.name(), |r| {
            r.bindings.push(binding(b"RSFExtra", 0, 12))
        });
        check(ExtraBinding.name(), |r| {
            r.bindings.last_mut().unwrap().point = 12
        });
        let mut wrong_stage = variant.clone();
        wrong_stage.bindings.last_mut().unwrap().name =
            Bytes::from(core_buffer(if stage == b'v' { b'p' } else { b'v' }));
        assert!(compatible(stage, &stock, &wrong_stage) == ExtraBinding);
        check(Buffer.name(), |r| r.buffers[0].variables[0].1 = 16);
        check(Buffer.name(), |r| r.buffers[1].size = 48);
        check(Version.name(), |r| r.version = 0);
    }
    let mut vs = core(b'v', true);
    vs.inputs[1].component = 1;
    assert!(compatible(b'v', &core(b'v', false), &vs) == Inputs);
    let mut vs = core(b'v', true);
    vs.outputs.remove(2);
    assert!(compatible(b'v', &core(b'v', false), &vs) == Outputs);
    let mut ps = core(b'p', true);
    ps.outputs[2] = target(3);
    assert!(compatible(b'p', &core(b'p', false), &ps) == Outputs);
    let mut ps = core(b'p', true);
    ps.outputs.push(param(b"SV_Depth", 0, u32::MAX, 1, 0));
    assert!(compatible(b'p', &core(b'p', false), &ps) == Outputs);
    assert!(linkable(&core(b'v', true), &core(b'p', true)));
    assert!(!linkable(&core(b'v', false), &core(b'p', true)));
    assert!(!linkable(&core(b'v', true), &core(b'p', false)));
}

fn controls(variant: bool) -> Vec<Parameter> {
    if variant {
        vec![
            tex(12, 0, 15),
            tex(13, 1, 1),
            tex(0, 2, 3),
            tex(1, 3, 15),
            tex(2, 4, 7),
            tex(14, 5, 15),
            tex(15, 6, 7),
        ]
    } else {
        vec![tex(0, 0, 3), tex(1, 1, 15), tex(2, 2, 7)]
    }
}

fn tess(stage: u8, variant: bool) -> TestReflection {
    let hull = stage == b'h';
    let mut r = TestReflection {
        version: if hull { 0x30050 } else { 0x40050 },
        bindings: vec![binding(b"$Globals", 0, 0)],
        buffers: vec![TestBuffer {
            name: b"$Globals",
            size: 32,
            variables: vec![(if hull { b"triLOD" } else { b"m_P" }, 0, 16)],
        }],
        inputs: controls(variant),
        outputs: if hull {
            controls(variant)
        } else if variant {
            vec![
                tex(12, 0, 15),
                tex(13, 1, 1),
                tex(0, 2, 3),
                tex(1, 3, 15),
                position(4, 15),
            ]
        } else {
            vec![tex(0, 0, 3), tex(1, 1, 15), position(2, 15)]
        },
        patch: vec![
            param(b"SV_TessFactor", 0, 0, 1, 13),
            param(b"POSITION", 4, 0, 14, 0),
            param(b"SV_InsideTessFactor", 0, 1, 1, 14),
            param(b"POSITION", 3, 2, 7, 0),
            param(b"NORMAL", 3, 3, 7, 0),
        ],
    };
    if variant && !hull {
        r.bindings.push(binding(b"RSFTessPrevious", 0, 13));
    }
    r
}

pub(in crate::shader_loader) fn tess_compatibility() {
    use Compatibility::*;
    for stage in [b'h', b'd'] {
        let stock = tess(stage, false);
        assert!(compatible(stage, &stock, &tess(stage, true)) == Ok);
        let check = |expected: Compatibility, edit: fn(&mut TestReflection)| {
            let mut r = tess(stage, true);
            edit(&mut r);
            assert_eq!(compatible(stage, &stock, &r).name(), expected.name());
        };
        check(PatchConstants, |r| r.patch[1].mask = 15);
        check(Inputs, |r| {
            r.inputs.pop();
        });
        check(Inputs, |r| r.inputs[6] = tex(12, 6, 15));
        check(Inputs, |r| r.inputs.push(tex(16, 7, 15)));
        check(Inputs, |r| r.inputs[0].mask = 7);
        check(Inputs, |r| {
            r.inputs.remove(3);
        });
        check(Buffer, |r| r.buffers[0].variables[0].1 = 16);
    }
    let mut hull = tess(b'h', true);
    hull.bindings.push(binding(b"RSFTessPrevious", 0, 13));
    assert!(compatible(b'h', &tess(b'h', false), &hull) == ExtraBinding);
    let ds = tess(b'd', false);
    let check = |expected: Compatibility, edit: fn(&mut TestReflection)| {
        let mut r = tess(b'd', true);
        edit(&mut r);
        assert_eq!(compatible(b'd', &ds, &r).name(), expected.name());
    };
    check(ExtraBinding, |r| {
        r.bindings[1].name = Bytes::from(b"RSFCorePrevious".as_slice())
    });
    check(ExtraBinding, |r| r.bindings[1].point = 12);
    check(Outputs, |r| r.outputs.push(tex(14, 5, 15)));
    check(Outputs, |r| {
        r.outputs.remove(1);
    });
    check(Outputs, |r| r.outputs[1] = tex(12, 1, 15));
    let mut hull = tess(b'h', true);
    hull.outputs.remove(0);
    assert!(compatible(b'h', &tess(b'h', false), &hull) == Outputs);
    let mut hull = tess(b'h', true);
    hull.outputs[0] = tex(13, 0, 1);
    assert!(compatible(b'h', &tess(b'h', false), &hull) == Outputs);
    assert!(compatible(b'g', &tess(b'h', false), &tess(b'h', true)) != Ok);
}

pub(in crate::shader_loader) fn tess_linkage() {
    let mut vs = TestReflection {
        outputs: controls(true),
        ..Default::default()
    };
    vs.outputs.push(position(7, 15));
    let ps = TestReflection {
        inputs: vec![
            tex(12, 0, 15),
            tex(13, 1, 1),
            tex(0, 2, 3),
            tex(1, 3, 15),
            position(4, 3),
        ],
        ..Default::default()
    };
    let hull = tess(b'h', true);
    let domain = tess(b'd', true);
    let link = |vs: &TestReflection, hs: &TestReflection, ds: &TestReflection| {
        rsf_anomaly_shader_tess_linkable(
            vs.snapshot().view(),
            hs.snapshot().view(),
            ds.snapshot().view(),
            ps.snapshot().view(),
        )
    };
    assert!(link(&vs, &hull, &domain));
    assert!(!link(&vs, &tess(b'h', false), &domain));
    assert!(!link(&vs, &hull, &tess(b'd', false)));
    let mut plain = vs.clone();
    plain.outputs.drain(5..7);
    assert!(!link(&plain, &hull, &domain));
    let mut shifted = hull.clone();
    shifted.inputs[5].register = 6;
    assert!(!link(&vs, &shifted, &domain));
    let mut unmatched = domain.clone();
    unmatched.patch.push(param(b"NORMAL", 4, 4, 7, 0));
    assert!(!link(&vs, &hull, &unmatched));
    unmatched.patch.clear();
    assert!(!link(&vs, &hull, &unmatched));
    assert!(!linkable(&vs, &ps));
    use crate::shader_family::{FAMILIES, FAMILY_COUNT, TESS_DOMAIN, TESS_HULL};
    assert_eq!(FAMILY_COUNT, 8);
    assert_eq!(FAMILIES[TESS_HULL as usize].2, b'h');
    assert_eq!(
        FAMILIES[TESS_DOMAIN as usize],
        (TESS_DOMAIN, c"tess_domain", b'd')
    );
    assert_eq!(Compatibility::PatchConstants.name(), b"patch_constants");
}

pub(in crate::shader_loader) fn foliage_layout() {
    use crate::motion::FoliageLayout;
    use crate::shader_family::{CORE_STATIC, FOLIAGE_DETAIL, FOLIAGE_TREE};

    let mut tree = TestReflection {
        buffers: vec![
            TestBuffer {
                name: b"dynamic_transforms",
                size: 256,
                variables: vec![(b"m_VP", 0, 64)],
            },
            TestBuffer {
                name: b"$Globals",
                size: 144,
                variables: vec![
                    (b"m_xform", 0, 48),
                    (b"consts", 48, 16),
                    (b"c_scale", 64, 16),
                    (b"wind", 80, 16),
                    (b"wave", 96, 16),
                ],
            },
        ],
        ..Default::default()
    };
    let layout = |family, reflection: &TestReflection| {
        FoliageLayout::from_stock(family, reflection.snapshot().view())
    };
    let full = layout(FOLIAGE_TREE, &tree);
    assert!(full.usable);
    assert_eq!(
        (
            full.xform.buffer,
            full.xform.offset,
            full.xform.size,
            full.xform.buffer_size
        ),
        (1, 0, 48, 144)
    );
    assert_eq!(
        (full.wave.offset, full.wind.offset, full.consts.offset),
        (96, 80, 48)
    );
    tree.buffers[1].variables.truncate(1);
    let no_wind = layout(FOLIAGE_TREE, &tree);
    assert!(
        no_wind.usable && !no_wind.wave.present && !no_wind.wind.present && !no_wind.consts.present
    );
    tree.buffers[1].variables = vec![(b"consts", 48, 16)];
    assert!(!layout(FOLIAGE_TREE, &tree).usable);
    tree.buffers[1].variables = vec![(b"m_xform", 120, 48)];
    assert!(!layout(FOLIAGE_TREE, &tree).usable);

    let mut detail = TestReflection {
        buffers: vec![TestBuffer {
            name: b"$Globals",
            size: 4048,
            variables: vec![
                (b"consts", 0, 16),
                (b"wave", 16, 16),
                (b"dir2D", 32, 16),
                (b"array", 48, 3904),
            ],
        }],
        ..Default::default()
    };
    let waved = layout(FOLIAGE_DETAIL, &detail);
    assert!(waved.usable && waved.wave.present);
    assert_eq!(
        (waved.array.offset, waved.array.size, waved.wind.offset),
        (48, 3904, 32)
    );
    detail.buffers[0].variables = vec![(b"consts", 0, 16), (b"array", 16, 3904)];
    let still = layout(FOLIAGE_DETAIL, &detail);
    assert!(still.usable && !still.wave.present && !still.wind.present);
    detail.buffers[0].variables = vec![(b"array", 16, 3000)];
    assert!(!layout(FOLIAGE_DETAIL, &detail).usable);
    assert!(!layout(CORE_STATIC, &detail).usable);
}

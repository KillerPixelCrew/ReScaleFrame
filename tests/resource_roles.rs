// SPDX-License-Identifier: MIT
// Resource facts from the 2048x1152 AC7 capture in docs/research/ac7-frame-capture.md.
use super::{TestResult, check};
use crate::sys::{
    RSF_ROLE_RULE_ANY_DEPTH_FORMAT as ANY_DEPTH, RSF_ROLE_RULE_FIXED_SIZE as FIXED,
    RSF_ROLE_RULE_RENDER_RANGE as RANGE, RSF_ROLE_RULE_RENDER_SIZED as RENDER,
    rsf_frame_shape as Shape, rsf_role_rule as Rule, rsf_role_table as Table,
    rsf_role_verdict as Verdict, rsf_texture_facts as Facts,
};
use std::ptr;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;

const OUT: [u32; 2] = [2048, 1152];
const HALF: [u32; 2] = [1024, 576];
const UNKNOWN: u32 = 0;
const SCENE: u32 = 1;
const DEPTH: u32 = 2;
const MOTION: u32 = 3;
const EXPOSURE: u32 = 4;
const HISTORY: u32 = 5;
const CONFIDENT: u32 = 0;
const CANDIDATE: u32 = 1;
const RTV: D3D11_BIND_FLAG = D3D11_BIND_RENDER_TARGET;
const SRV: D3D11_BIND_FLAG = D3D11_BIND_SHADER_RESOURCE;
const DSV: D3D11_BIND_FLAG = D3D11_BIND_DEPTH_STENCIL;
const UAV: D3D11_BIND_FLAG = D3D11_BIND_UNORDERED_ACCESS;

unsafe extern "C" {
    fn rsf_classify_texture(
        table: *const Table,
        facts: *const Facts,
        shape: *const Shape,
        verdict: *mut Verdict,
    ) -> u32;
}

fn facts([width, height]: [u32; 2], format: DXGI_FORMAT, bind: D3D11_BIND_FLAG) -> Facts {
    Facts {
        struct_size: size_of::<Facts>() as u32,
        width,
        height,
        format: format.0 as u32,
        bind_flags: bind.0 as u32,
        mip_levels: 1,
        array_size: 1,
        sample_count: 1,
    }
}

fn shape([render_width, render_height]: [u32; 2]) -> Shape {
    Shape {
        struct_size: size_of::<Shape>() as u32,
        output_width: OUT[0],
        output_height: OUT[1],
        render_width,
        render_height,
    }
}

fn rule(
    role: u32,
    confidence: u32,
    flags: u32,
    format: DXGI_FORMAT,
    bind: D3D11_BIND_FLAG,
    [width, height]: [u32; 2],
) -> Rule {
    Rule {
        role,
        confidence,
        flags,
        format: format.0 as u32,
        bind_flags: bind.0 as u32,
        width,
        height,
    }
}

fn table(rules: &[Rule]) -> Table {
    Table {
        struct_size: size_of::<Table>() as u32,
        rules: rules.as_ptr(),
        count: rules.len() as u32,
    }
}

fn verdict(role: u32) -> Verdict {
    Verdict {
        struct_size: size_of::<Verdict>() as u32,
        role,
        confidence: CONFIDENT,
    }
}

fn classify(
    table: &Table,
    facts: Facts,
    shape: &Shape,
    expected: (u32, u32),
    what: &str,
) -> TestResult {
    let mut result = verdict(UNKNOWN);
    check(
        unsafe { rsf_classify_texture(table, &facts, shape, &mut result) } == 1,
        &format!("{what}: classification refused"),
    )?;
    let actual = (result.role, result.confidence);
    check(
        actual == expected,
        &format!("{what}: got {actual:?}, expected {expected:?}"),
    )
}

fn run() -> TestResult {
    let rules = [
        rule(
            EXPOSURE,
            CONFIDENT,
            FIXED,
            DXGI_FORMAT_R32G32_FLOAT,
            D3D11_BIND_FLAG(0),
            [1, 1],
        ),
        rule(
            MOTION,
            CONFIDENT,
            RENDER,
            DXGI_FORMAT_R16G16_UNORM,
            RTV,
            [0, 0],
        ),
        rule(
            DEPTH,
            CONFIDENT,
            RENDER | ANY_DEPTH,
            DXGI_FORMAT_UNKNOWN,
            DSV,
            [0, 0],
        ),
        rule(
            HISTORY,
            CANDIDATE,
            RENDER,
            DXGI_FORMAT_R16G16B16A16_FLOAT,
            RTV | SRV,
            [0, 0],
        ),
        rule(
            SCENE,
            CANDIDATE,
            RENDER,
            DXGI_FORMAT_R11G11B10_FLOAT,
            RTV,
            [0, 0],
        ),
    ];
    let observed = table(&rules);
    let class = |facts, shape, role, confidence, what| {
        classify(&observed, facts, shape, (role, confidence), what)
    };
    let unknown = |facts, shape, what| class(facts, shape, UNKNOWN, CONFIDENT, what);
    let full = shape(OUT);
    let halved = shape(HALF);
    let uncertain = shape([0, 0]);
    let motion = facts(OUT, DXGI_FORMAT_R16G16_UNORM, RTV | SRV);
    let half_motion = facts(HALF, DXGI_FORMAT_R16G16_UNORM, RTV | SRV);
    let third = facts(
        [OUT[0] / 3, OUT[1] / 3],
        DXGI_FORMAT_R16G16_UNORM,
        RTV | SRV,
    );
    let quarter = facts(
        [OUT[0] / 4, OUT[1] / 4],
        DXGI_FORMAT_R16G16_UNORM,
        RTV | SRV,
    );
    let mask = facts(HALF, DXGI_FORMAT_R16G16_UNORM, UAV | SRV);
    class(motion, &full, MOTION, CONFIDENT, "resource 2163")?;
    unknown(mask, &halved, "UAV mask is distinct from velocity")?;
    unknown(mask, &uncertain, "unknown extent still rejects UAV mask")?;
    class(half_motion, &halved, MOTION, CONFIDENT, "half-size motion")?;
    unknown(half_motion, &full, "known full render extent is exact")?;
    class(
        half_motion,
        &uncertain,
        MOTION,
        CONFIDENT,
        "unknown extent admits half size",
    )?;
    class(
        motion,
        &uncertain,
        MOTION,
        CONFIDENT,
        "unknown extent admits full size",
    )?;
    unknown(quarter, &uncertain, "default minimum is half output")?;

    for format in [
        DXGI_FORMAT_R32G8X24_TYPELESS,
        DXGI_FORMAT_D32_FLOAT_S8X24_UINT,
        DXGI_FORMAT_R32_TYPELESS,
        DXGI_FORMAT_D32_FLOAT,
        DXGI_FORMAT_R24G8_TYPELESS,
        DXGI_FORMAT_D24_UNORM_S8_UINT,
        DXGI_FORMAT_R24_UNORM_X8_TYPELESS,
    ] {
        class(
            facts(OUT, format, DSV | SRV),
            &full,
            DEPTH,
            CONFIDENT,
            "depth-stencil families",
        )?;
    }
    unknown(
        facts(OUT, DXGI_FORMAT_R24G8_TYPELESS, SRV),
        &full,
        "depth needs DSV flag",
    )?;
    for dimensions in [[2048, 2048], [5120, 1024], [1024, 1024]] {
        let cascade = facts(dimensions, DXGI_FORMAT_R32G8X24_TYPELESS, DSV);
        unknown(cascade, &full, "shadow cascade at full extent")?;
        unknown(
            cascade,
            &uncertain,
            "unknown extent still rejects shadow cascade",
        )?;
        unknown(cascade, &halved, "shadow cascade at half extent")?;
    }
    class(
        facts([1440, 808], DXGI_FORMAT_R32G8X24_TYPELESS, DSV),
        &uncertain,
        DEPTH,
        CONFIDENT,
        "alignment padding permits a small aspect difference",
    )?;

    let exposure = facts([1, 1], DXGI_FORMAT_R32G32_FLOAT, RTV | SRV);
    let no_extent = Shape {
        output_width: 0,
        output_height: 0,
        ..uncertain
    };
    class(exposure, &full, EXPOSURE, CONFIDENT, "exposure")?;
    class(
        exposure,
        &no_extent,
        EXPOSURE,
        CONFIDENT,
        "fixed exposure ignores extent",
    )?;
    unknown(
        motion,
        &no_extent,
        "no dimensions means no render-size match",
    )?;
    let history = facts(OUT, DXGI_FORMAT_R16G16B16A16_FLOAT, RTV | SRV);
    for matched in [history, history] {
        class(
            matched,
            &full,
            HISTORY,
            CANDIDATE,
            "identical history shapes are candidates",
        )?;
    }
    unknown(
        facts(OUT, DXGI_FORMAT_R16G16B16A16_FLOAT, RTV),
        &full,
        "history must be sampleable",
    )?;
    unknown(
        facts(OUT, DXGI_FORMAT_B8G8R8A8_TYPELESS, RTV | SRV),
        &full,
        "ordinary colour",
    )?;
    let scene = facts(OUT, DXGI_FORMAT_R11G11B10_FLOAT, RTV | SRV);
    class(
        scene,
        &full,
        SCENE,
        CANDIDATE,
        "R11G11B10 pre-tonemap candidate",
    )?;
    unknown(
        facts(OUT, DXGI_FORMAT_R11G11B10_FLOAT, SRV),
        &full,
        "scene requires RTV flag",
    )?;
    unknown(
        Facts {
            mip_levels: 3,
            ..scene
        },
        &full,
        "mipped scene refused",
    )?;
    for invalid in [
        Facts {
            sample_count: 4,
            ..motion
        },
        Facts {
            mip_levels: 9,
            ..history
        },
        Facts {
            mip_levels: 0,
            ..history
        },
        Facts {
            array_size: 6,
            ..motion
        },
    ] {
        unknown(invalid, &full, "not a single plain surface")?;
    }
    class(
        Facts {
            struct_size: motion.struct_size + 8,
            ..motion
        },
        &full,
        MOTION,
        CONFIDENT,
        "extended facts",
    )?;

    let wide_rule = [Rule {
        flags: RENDER | RANGE,
        width: 3,
        height: 3,
        ..rules[1]
    }];
    let wide = table(&wide_rule);
    let third_extent = shape([third.width, third.height]);
    classify(
        &wide,
        third,
        &uncertain,
        (MOTION, CANDIDATE),
        "adapter range admits one third",
    )?;
    classify(
        &wide,
        third,
        &third_extent,
        (MOTION, CONFIDENT),
        "known size preserves confidence",
    )?;
    classify(
        &wide,
        third,
        &halved,
        (UNKNOWN, CONFIDENT),
        "known extent overrides range",
    )?;
    classify(
        &wide,
        quarter,
        &uncertain,
        (UNKNOWN, CONFIDENT),
        "below adapter minimum",
    )?;

    let short_facts = Facts {
        struct_size: motion.struct_size - 4,
        ..motion
    };
    let short_shape = Shape {
        struct_size: full.struct_size - 4,
        ..full
    };
    let short_table = Table {
        struct_size: observed.struct_size - 4,
        ..observed
    };
    let no_rules = Table {
        rules: ptr::null(),
        ..observed
    };
    let mut result = verdict(DEPTH);
    for (table, facts, shape) in [
        (
            &observed as *const Table,
            &short_facts as *const Facts,
            &full as *const Shape,
        ),
        (&observed, &motion, &short_shape),
        (&short_table, &motion, &full),
        (&no_rules, &motion, &full),
        (ptr::null(), &motion, &full),
        (&observed, ptr::null(), &full),
        (&observed, &motion, ptr::null()),
    ] {
        check(
            unsafe { rsf_classify_texture(table, facts, shape, &mut result) } == 0
                && result.role == DEPTH,
            "invalid inputs leave verdict untouched",
        )?;
    }
    check(
        unsafe { rsf_classify_texture(&observed, &motion, &full, ptr::null_mut()) } == 0,
        "null verdict refuses",
    )?;
    result.struct_size -= 4;
    check(
        unsafe { rsf_classify_texture(&observed, &motion, &full, &mut result) } == 0
            && result.role == DEPTH,
        "short verdict refuses without writing",
    )?;

    classify(
        &table(&[]),
        motion,
        &full,
        (UNKNOWN, CONFIDENT),
        "empty table",
    )?;
    let ordered = [
        rule(
            MOTION,
            CANDIDATE,
            FIXED,
            DXGI_FORMAT_R16G16_UNORM,
            D3D11_BIND_FLAG(0),
            [640, 360],
        ),
        rule(
            DEPTH,
            CONFIDENT,
            0,
            DXGI_FORMAT_R16G16_UNORM,
            D3D11_BIND_FLAG(0),
            [0, 0],
        ),
    ];
    let priority = table(&ordered);
    classify(
        &priority,
        facts([640, 360], DXGI_FORMAT_R16G16_UNORM, D3D11_BIND_FLAG(0)),
        &full,
        (MOTION, CANDIDATE),
        "first matching rule wins",
    )?;
    classify(
        &priority,
        facts([641, 360], DXGI_FORMAT_R16G16_UNORM, D3D11_BIND_FLAG(0)),
        &full,
        (DEPTH, CONFIDENT),
        "miss falls through",
    )
}

#[unsafe(no_mangle)]
extern "C" fn rsf_resource_roles_fixture() -> i32 {
    match run() {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("Resource roles: {error}");
            1
        }
    }
}

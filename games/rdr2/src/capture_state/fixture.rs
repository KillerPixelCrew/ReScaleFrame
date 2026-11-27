// SPDX-License-Identifier: MIT
//! Artifact checks for the WARP/NGX capture driver.
mod driver;
use std::ffi::CStr;
use std::fs;
use std::path::Path;
use std::thread::sleep;
use std::time::Duration;
use windows_sys::Win32::System::Threading::GetCurrentProcessId;

fn require(ok: bool, message: &'static CStr) -> Result<(), &'static CStr> {
    ok.then_some(()).ok_or(message)
}

fn check_bundle(root: &Path, pid: u32) -> Result<(), &'static CStr> {
    let manifest = root.join(format!("bundle-{pid}-001.json"));
    for _ in 0..100 {
        if manifest.exists() {
            break;
        }
        sleep(Duration::from_millis(100));
    }
    let bundle = fs::read_to_string(manifest).map_err(|_| c"capture manifest written")?;
    for (field, message) in [
        ("\"draw_count\":75", c"full draw sequence"),
        ("\"index\":3", c"four temporal input groups"),
        ("\"write_failed\":false", c"complete writes"),
        ("\"readbacks_unavailable\":0", c"complete readback receipts"),
        (
            "\"capture_timeout\":false",
            c"capture completed without timeout",
        ),
        ("\"color_space_set\":true", c"observed color space"),
        (
            "\"pipeline_known\":true,\"ps_crc32\":\"",
            c"stream shader identity captured",
        ),
    ] {
        require(bundle.contains(field), message)?;
    }
    require(
        !bundle.contains("\"ps_crc32\":\"00000000\""),
        c"stream shader CRC present",
    )
}

fn check_images(root: &Path, pid: u32, rgb10: bool) -> Result<(), &'static CStr> {
    let final_path = root.join(format!("native-{pid}-001-074"));
    let metadata = fs::read_to_string(final_path.with_extension("json"))
        .map_err(|_| c"final draw metadata written")?;
    require(
        metadata.contains("\"pipeline_known\":true"),
        c"draw image reports observed pipeline",
    )?;
    let final_draw =
        fs::read(final_path.with_extension("bin")).map_err(|_| c"final draw retained past 64")?;
    require(
        final_draw.len() == 256 * 48,
        c"final draw readback dimensions",
    )?;
    if rgb10 {
        let packed =
            u32::from_le_bytes([final_draw[0], final_draw[1], final_draw[2], final_draw[3]]);
        require(
            [
                packed & 1023,
                (packed >> 10) & 1023,
                (packed >> 20) & 1023,
                packed >> 30,
            ] == [256, 512, 767, 3],
            c"actual final RGB10 pixels",
        )?;
    } else {
        require(
            final_draw.starts_with(&[191, 128, 64]),
            c"actual final BGRA pixels",
        )?;
    }
    let input = |role| root.join(format!("input-{pid}-001-000-{role}.bin"));
    let motion = fs::read(input("motion")).map_err(|_| c"motion readback written")?;
    require(motion.len() == 256 * 24, c"motion readback dimensions")?;
    require(
        motion.starts_with(&[0, 0x30, 0, 0xb4]),
        c"raw RG16F motion preserved",
    )?;
    let depth = fs::read(input("depth")).map_err(|_| c"depth plane readback written")?;
    require(depth.len() >= 256 * 24, c"depth plane readback dimensions")?;
    require(
        depth.starts_with(&0.25f32.to_le_bytes()),
        c"raw depth preserved",
    )
}

fn verify(root: &Path, rgb10: bool) -> Result<(), &'static CStr> {
    let pid = unsafe { GetCurrentProcessId() };
    check_bundle(root, pid).and_then(|()| check_images(root, pid, rgb10))
}

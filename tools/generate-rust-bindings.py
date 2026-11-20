"""Regenerate the Windows x64 C ABI bindings using bindgen-cli 0.72.1."""
import os
import subprocess
from pathlib import Path
from tempfile import NamedTemporaryFile

root = Path(__file__).resolve().parents[1]
bindgen = os.environ.get("BINDGEN", "bindgen")
if subprocess.check_output([bindgen, "--version"], text=True).strip() != "bindgen 0.72.1":
    raise SystemExit("Install bindgen-cli 0.72.1 to regenerate the SDK bindings")

def generate(output, header, types, constants, clang_args, extra=(), *, use_core=True):
    bindings = subprocess.check_output([
        bindgen, header, "--allowlist-type", types, "--allowlist-var", constants,
        "--raw-line", "#![allow(non_camel_case_types)]",
        "--no-derive-debug", *(["--use-core"] if use_core else []),
        "--no-doc-comments", "--rust-edition", "2024",
        "--rust-target", "1.85", *extra, "--", "--target=x86_64-pc-windows-msvc", *clang_args,
    ], cwd=root).replace(b"\r\n", b"\n")
    output = root / output
    if not output.exists() or output.read_bytes() != bindings:
        with NamedTemporaryFile(dir=output.parent, delete=False) as temporary:
            temporary.write(bindings)
        Path(temporary.name).replace(output)


generate("sdk/game/src/lib.rs", "sdk/game/include/rescaleframe/game_api.h",
         "rsf_(get_game_plugin_api_fn|frame_record|game_cpu_(source|receipt)_event)", "RSF_.*", ["-Isdk/game/include"])
generate("runtime/orchestrator/src/render_abi.rs", "runtime/orchestrator/include/rescaleframe/frame_assembly.h",
         "rsf_(frame_resources|pipeline_camera_frame|dlss_frame|fg_choice|render_link|sr_session_setup|sr_session_status|sr_provider|fg_host|fg_session_status|generation_provider|native_fg_status|motion_resolve_params|render_settings|sharpening_options)",
         "RSF_(FRAME_ASSEMBLY_ABI_VERSION|DLSS_ABI_VERSION|FG_(BACKEND_(AUTO|ALL)|ABI_VERSION|OFF|FIXED|AUTO|DYNAMIC|STAT_(TOTAL_PRESENTED|GENERATED_PRESENTED|PRESENT_CALLBACKS|ACTIVITY))|NATIVE_FG_REASON_NONE|PACING_(NONE|REFLEX|XELL)|REFLEX_(OFF|BOOST)|RENDER_LINK_ABI_VERSION|SR_SESSION_ABI_VERSION|BACKEND_ABI_VERSION|API_D3D12|SR_(FSR2|XESS)|RENDER_FRONTEND_ABI_VERSION|SHARPENING_ABI_VERSION)",
         ["-include", "rescaleframe/fg_choice.h", "-include", "rescaleframe/render_links.h",
          "-include", "rescaleframe/render_frontend.h",
          "-include", "rescaleframe/sr_session.h", "-include", "rescaleframe/fg_session.h",
          "-include", "rescaleframe/native_fg.h", "-include", "rescaleframe/motion_resolve.h", "-Iruntime/orchestrator/include",
          "-Iruntime/contract/include", "-Iruntime/backends/dlss/include", "-Iruntime/graphics/include",
          "-Iruntime/presentation/include", "-Isdk/game/include"],
         ["--blocklist-type", "rsf_(frame_record|frame_id|frame_phase|screen_class|camera_frame)",
          "--raw-line", "use rsf_game_sys::rsf_frame_record;"])
generate("runtime/orchestrator/src/vulkan_fg_abi.rs", "runtime/orchestrator/src/native_fg_vulkan_platform.h",
         "rsf_fgvk(_(setup|frame|status|host_options|latency_status|provider|platform))?",
         "RSF_FGVK_.*",
         ["-Iruntime/orchestrator/include", "-Iruntime/contract/include", "-Iruntime/graphics/include", "-Isdk/game/include"],
         ["--no-recursive-allowlist", "--raw-line", "use crate::vulkan_sr_abi::{rsf_srvk_image, rsf_srvk_setup};",
          "--raw-line", "use rsf_game_sys::rsf_frame_record;", "--raw-line", "type rsf_backend_result = i32;"])
generate("runtime/orchestrator/src/cpu_abi.rs", "runtime/orchestrator/include/rescaleframe/native_cpu.h",
         "rsf_native_(cpu_(frame|counters|reservation|reservation_counters)|input_receipts)",
         "RSF_NATIVE_CPU_(RESERVATION_.*|DEPENDENCY_.*|PRESENT_.*)",
         ["-Iruntime/orchestrator/include", "-Isdk/game/include"])
generate("runtime/orchestrator/src/scene_abi.rs", "runtime/orchestrator/include/rescaleframe/native_window.h",
         "rsf_(native_present_record|observer_present_event|native_scene_identity)",
         "RSF_NATIVE_SCENE_PRESENT_.*",
         ["-include", "rescaleframe/native_scene.h", "-Iruntime/orchestrator/include",
          "-Iruntime/graphics/include", "-Isdk/game/include"],
         ["--with-derive-default"])
generate("runtime/orchestrator/src/d3d11_output_abi.rs", "runtime/orchestrator/src/d3d11_output_platform.h",
         "rsf_(native_composition_frame|d3d11_reinsert_kernels)", "RSF_D3D11_(FORMAT_.*|USAGE_.*|BIND_.*)",
         ["-include", "rescaleframe/native_composition.h", "-Iruntime/orchestrator/include",
          "-Iruntime/graphics/include", "-Isdk/game/include"],
         ["--with-derive-default", "--no-copy", "rsf_d3d11_state",
          "--allowlist-function", "rsf_d3d11_output_.*"])
generate("runtime/orchestrator/src/auxiliary_sr_abi.rs", "runtime/orchestrator/src/auxiliary_sr_platform.h",
         "rsf_(aux_platform|d3d11_auxiliary_surface)",
         "RSF_(AUX_.*|D3D11_AUXILIARY_.*|DEPTH_SCALE_(ABI_VERSION|OK))",
         ["-Iruntime/orchestrator/include", "-Iruntime/graphics/include",
          "-Iruntime/presentation/include", "-Isdk/game/include"],
         ["--blocklist-type", "rsf_game_log_fn",
          "--raw-line", "use rsf_game_sys::rsf_game_log_fn;", "--with-derive-default"])
generate("runtime/orchestrator/src/native_sr_abi.rs", "runtime/orchestrator/include/rescaleframe/native_sr.h",
         "rsf_(dlss_pipeline_frame|native_translucency_masks|motion_decode_params)", "RSF_DLSS_PIPELINE_ABI_VERSION",
         ["-include", "rescaleframe/native_translucency.h", "-Iruntime/orchestrator/include",
          "-Iruntime/backends/dlss/include", "-Iruntime/graphics/include", "-Iruntime/contract/include",
          "-Isdk/game/include"],
         ["--blocklist-type", "rsf_pipeline_camera_frame",
          "--raw-line", "use crate::render_abi::rsf_pipeline_camera_frame;"])
# Match the overlay's existing std paths and import types that other modules already own.
generate("runtime/orchestrator/src/overlay_abi.rs", "runtime/orchestrator/src/overlay_platform.h",
         "wchar_t|rsf_overlay_(api|service_setup|service_option|capture|diagnostics|sr_report|stats|intent|"
         "host_option|input_counters|platform|quality)|rsf_native_(latency_status|input_routes)|rsf_render_status",
         "RSF_OVERLAY_(API_VERSION|CAPTURE_.*|NOTIFY_.*|SR_INPUT_.*|SERVICE_OPTION_(TOGGLE|NUMBER|LIMIT)|"
         "HOST_OPTION_(TOGGLE|NUMBER)|FG_RUNTIME_SWITCH)|RSF_NATIVE_FG_REASON_(RESET|SCENE_UNSUPPORTED|"
         "HUDLESS_MISSING)|RSF_(REFLEX_ON|SR_NONE|SR_DLSS|SR_FSR3|SR_FSR4|SR_SPATIAL)",
         ["-include", "rescaleframe/overlay_api.h", "-include", "rescaleframe/overlay.h",
          "-include", "rescaleframe/overlay_input.h", "-include", "rescaleframe/native_fg.h",
          "-include", "rescaleframe/native_cpu.h", "-include", "rescaleframe/sr_session.h",
          "-Isdk/game/include", "-Iruntime/orchestrator/include", "-Iruntime/contract/include",
          "-Iruntime/presentation/include", "-Iruntime/graphics/include",
          "-Iruntime/backends/dlss/include", "-Iui/overlay/include"],
         ["--no-recursive-allowlist", "--with-derive-default",
          "--raw-line", "use crate::cpu_abi::rsf_native_cpu_counters;",
          "--raw-line", "use crate::render_abi::{\n"
                        "    rsf_generation_provider, rsf_native_fg_options, rsf_native_fg_status, rsf_render_settings,\n"
                        "    rsf_sharpening_options,\n"
                        "};",
          "--raw-line", "use rsf_game_sys::rsf_game_log_fn;"],
         use_core=False)
generate("runtime/orchestrator/src/vulkan_sr_abi.rs", "runtime/orchestrator/include/rescaleframe/native_sr_split_vulkan.h",
         "rsf_(srvk_(setup|frame|image|fallback|fsr4_provider_info)|fsr4vk_handoff|vk_(function|get_proc))",
         "RSF_(SRVK_(ABI_VERSION|SPLIT_ABI_VERSION|SPLIT_TRACKED_ABI_VERSION|FSR4_INFO_ABI_VERSION)|SR_(DLSS|FSR2|FSR3|FSR4|XESS)|FSR4_BRIDGE_(INVALID_ARGUMENT|ABI_MISMATCH))",
         ["-include", "runtime/vulkan/fsr4/bridge_api.h", "-Iruntime/orchestrator/include",
          "-Iruntime/contract/include", "-Iruntime/graphics/include", "-Isdk/game/include"],
         ["--no-recursive-allowlist", "--raw-line", "use crate::render_abi::rsf_sharpening_options;",
          "--raw-line", "use rsf_game_sys::rsf_frame_record;"])
generate("runtime/orchestrator/src/gl_pipeline_abi.rs", "runtime/orchestrator/include/rescaleframe/gl_pipeline.h",
         "rsf_(gl_pipeline_(setup|options|surfaces|status)|gl_texture_info|sr12_setup|dlss_setup)",
         "RSF_(GL_(RGBA16F|R32F|RG16F|RGBA8)|UI_MODE_BACKBUFFER_HUDLESS)",
         ["-include", "rescaleframe/native_sr_d3d12.h", "-include", "rescaleframe/native_fg_d3d12.h",
          "-Iruntime/orchestrator/include", "-Iruntime/backends/dlss/include", "-Iruntime/graphics/include",
          "-Iruntime/contract/include", "-Iruntime/presentation/include", "-Isdk/game/include"],
         ["--blocklist-type", "rsf_(native_fg_options|native_fg_status|sharpening_options|backend_log_fn|fg_options|fg_status|generation_provider|generation_setup|fg_mode|reflex_mode|pacing_owner|fg_retirement|quality|ui_mode|vendor|gfx_api|latency_mode|latency_marker|lifetime|backend_result|sr_backend)",
          "--raw-line", "use crate::render_abi::rsf_backend_log_fn;",
          "--raw-line", "use crate::render_abi::rsf_native_fg_options;",
          "--raw-line", "use crate::render_abi::rsf_native_fg_status;",
          "--raw-line", "use crate::render_abi::rsf_sharpening_options;",
          "--with-derive-default",
          "--with-derive-custom-struct", "rsf_(gl_pipeline_(setup|options|status)|sr12_setup)=Copy,Clone"])
generate("runtime/orchestrator/src/fg12_abi.rs", "runtime/orchestrator/src/fg12_platform.h",
         "rsf_(fg12_(setup|texture_desc|readback_layout|calls)|d3d11_present_setup|streamline_host|native_latency_frame|"
         "observer_present(_event)?_fn)",
         "RSF_(NATIVE_FG_REASON_(NOT_PREPARED|SCENE_UNSUPPORTED|RESET|HUDLESS_MISSING)|UI_MODE_NONE|GAME_LATENCY_(SLEEP|MODE))",
         ["-include", "rescaleframe/fg_choice.h", "-Iruntime/orchestrator/include", "-Iruntime/contract/include",
          "-Iruntime/backends/dlss/include", "-Iruntime/graphics/include", "-Iruntime/presentation/include",
          "-Isdk/game/include"],
         ["--no-recursive-allowlist",
          "--raw-line", "use crate::render_abi::{\n"
                        "    rsf_backend_log_fn, rsf_backend_result, rsf_fg_frame, rsf_generation_provider,\n"
                        "    rsf_native_fg_options,\n"
                        "};",
          "--raw-line", "use crate::{overlay_abi::rsf_native_latency_status, scene_abi::rsf_observer_present_event};",
          "--raw-line", "use rsf_game_sys::{\n"
                        "    rsf_frame_record, rsf_game_cpu_event, rsf_game_cpu_event_fn, rsf_game_render_pass,\n"
                        "};"])
generate("runtime/orchestrator/src/native_fg_abi.rs", "runtime/orchestrator/src/native_fg_platform.h",
         "rsf_nfg_(platform|texture_info|cpu_owner)", "RSF_NFG_.*",
         ["-Iruntime/orchestrator/include", "-Iruntime/contract/include", "-Iruntime/backends/dlss/include",
          "-Iruntime/graphics/include", "-Iruntime/presentation/include", "-Isdk/game/include"],
         ["--no-recursive-allowlist",
          "--raw-line", "use crate::cpu_abi::rsf_native_cpu_frame;",
          "--raw-line", "use crate::render_abi::{\n"
                        "    rsf_backend_log_fn, rsf_dlss_frame, rsf_fg_frame, rsf_fg_status, rsf_generation_provider,\n"
                        "};",
          "--raw-line", "use crate::scene_abi::rsf_native_scene_identity;",
          "--raw-line", "use rsf_game_sys::{rsf_frame_record, rsf_game_render_pass};"])

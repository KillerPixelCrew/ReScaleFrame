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
         "rsf_(get_game_plugin_api_fn|frame_record|game_cpu_(source|receipt)_event)", "RSF_.*",
         ["-Isdk/game/include"], ["--raw-line", "pub mod sr_abi;"])
generate("ui/overlay/src/host_option_limit.rs", "runtime/orchestrator/include/rescaleframe/overlay_api.h",
         "NOTHING", "RSF_OVERLAY_SERVICE_OPTION_LIMIT",
         ["-Isdk/game/include", "-Iruntime/orchestrator/include", "-Iruntime/contract/include",
          "-Iruntime/presentation/include", "-Iruntime/graphics/include", "-Iruntime/backends/dlss/include"])
generate("sdk/game/src/sr_abi.rs", "runtime/orchestrator/include/rescaleframe/d3d11_sr_api.h",
         "rsf_(d3d11_sr_api|get_d3d11_sr_api_fn|dlss_(pipeline_(setup|status|result|log_fn)|quality|engine)|"
         "motion_decode_params|overlay_sr_report)",
         "RSF_(D3D11_SR_API_VERSION|DLSS_PIPELINE_ABI_VERSION|OVERLAY_SR_INPUT_.*)",
         ["-include", "rescaleframe/overlay_api.h", "-Isdk/game/include", "-Iruntime/orchestrator/include",
          "-Iruntime/contract/include", "-Iruntime/graphics/include", "-Iruntime/presentation/include",
          "-Iruntime/backends/dlss/include", "-Iui/overlay/include"],
         ["--no-recursive-allowlist", "--with-derive-default",
          "--raw-line", "use crate::{rsf_game_render_pass, rsf_result};",
          "--raw-line", "type rsf_sharpening_options = ::core::ffi::c_void;"])
generate("games/anomaly/src/fg_inputs_abi.rs", "games/anomaly/src/fg_inputs_platform.h",
         "rsf_anomaly_input_(log|frame|desc|targets|constants|platform)", "NOTHING", [],
         ["--no-recursive-allowlist", "--with-derive-default"])
generate("games/anomaly/src/fg_policy_abi.rs", "games/anomaly/src/fg_policy_platform.h",
         "rsf_anomaly_fg_(decision|counters)", "RSF_ANOMALY_(GAMEPLAY_SCREEN|HELD_SCREEN|SKIP_.*|RESET_.*)",
         ["-Isdk/game/include"], ["--no-recursive-allowlist", "--with-derive-default"])
generate("games/anomaly/src/fg_stage_abi.rs", "games/anomaly/src/fg_stage_platform.h",
         "rsf_(anomaly_fg_.*|d3d11_(present_(api|setup)|fg_api)|get_d3d11_(present|fg)_api_fn|"
         "native_fg_(options|status)|fg_status|dlss_frame|observer_present(_event)?(_fn)?|backend_log_fn)",
         "RSF_(D3D11_(PRESENT|FG)_API_(VERSION|ENTRY_POINT)|DLSS_ABI_VERSION|"
         "FG_(BACKEND_.*|OFF|FIXED)|REFLEX_(ON|BOOST)|UI_MODE_BACKBUFFER_HUDLESS)",
         ["-Isdk/game/include", "-Iruntime/orchestrator/include", "-Iruntime/contract/include",
          "-Iruntime/graphics/include", "-Iruntime/presentation/include", "-Iruntime/backends/dlss/include"],
         ["--no-recursive-allowlist", "--with-derive-default", "--raw-line", "#![allow(dead_code)]",
          "--raw-line", "use crate::fg_inputs_abi::rsf_anomaly_input_platform;",
          "--raw-line", "use rsf_game_sys::sr_abi::rsf_dlss_quality;",
          "--raw-line", "use rsf_game_sys::{rsf_game_cpu_event, rsf_game_render_pass, rsf_result};",
          "--raw-line", "type wchar_t = u16;",
          "--raw-line", "type rsf_streamline_host = ::core::ffi::c_void;",
          "--raw-line", "type rsf_streamline_graphics = ::core::ffi::c_void;",
          "--raw-line", "type rsf_generation_provider = ::core::ffi::c_void;",
          "--raw-line", "type rsf_latency_marker = u32;",
          "--raw-line", "type rsf_fg_mode = u32;",
          "--raw-line", "type rsf_reflex_mode = u32;",
          "--raw-line", "type rsf_pacing_owner = u32;"])
generate("games/anomaly/src/geometry_registry_abi.rs", "games/anomaly/src/geometry_registry_platform.h",
         "rsf_anomaly_geometry_(draw|key|probe|retired)",
         "RSF_ANOMALY_(VISUAL_.*|PASS_SHADOW|TYPE_(SKELETON_.*|TREE_.*)|DRAW_(IDENTIFIED.*|IGNORED_.*|UNIDENTIFIED_.*))", [],
         ["--no-recursive-allowlist", "--with-derive-default", "--raw-line", "#![allow(dead_code)]",
          "--with-derive-custom-struct", "rsf_anomaly_geometry_key=Eq,Ord,PartialEq,PartialOrd"])
generate("games/anomaly/src/motion_target_abi.rs", "games/anomaly/src/motion_target_platform.h",
         "rsf_anomaly_motion_bind_(state|layout|platform)", "RSF_ANOMALY_MOTION_BIND_.*", [],
         ["--no-recursive-allowlist", "--with-derive-default"])
generate("games/anomaly/src/motion_device_abi.rs", "games/anomaly/src/motion_device_platform.h",
         "rsf_anomaly_motion_(device_(platform|view)|log)", "RSF_MOTION_.*", [],
         ["--with-derive-default"])
generate("games/anomaly/src/native_observer_abi.rs", "games/anomaly/src/native_observer_platform.h",
         "rsf_anomaly_(native_observation|observer_(engine|frame|camera|platform))",
         "RSF_ANOMALY_OBSERVATION_ABI", ["-Isdk/game/include"],
         ["--no-recursive-allowlist", "--with-derive-default",
          "--raw-line", "use rsf_game_sys::rsf_game_log_fn;"])
generate("games/anomaly/src/overlay_abi.rs", "games/anomaly/src/overlay_platform.h",
         "wchar_t|rsf_anomaly_overlay_(option|sr_settings|sr_choices|fg|chain|platform|options)|"
         "rsf_overlay_(api|service_setup|service_option|capture|diagnostics)|rsf_named_frame_dump(_report|_log_fn)?|"
         "rsf_render_(settings|status)|rsf_get_overlay_(api|diagnostics)_fn|rsf_input_dump_named_texture_fn",
         "RSF_ANOMALY_OVERLAY_.*|RSF_OVERLAY_(API_VERSION|API_ENTRY_POINT|DIAGNOSTICS_ENTRY_POINT|CAPTURE_.*|"
         "NOTIFY_.*|SERVICE_OPTION_(TOGGLE|NUMBER|LIMIT))|RSF_(NAMED_FRAME_DUMP_ABI_VERSION|"
         "INPUT_DUMP_NAMED_TEXTURE_ENTRY_POINT|RENDER_FRONTEND_ABI_VERSION|SR_NONE|FG_BACKEND_(AUTO|ALL)|"
         "REFLEX_(OFF|ON|BOOST))",
         ["-include", "rescaleframe/sr_session.h", "-include", "rescaleframe/frame_generation.h",
          "-Isdk/game/include", "-Iruntime/orchestrator/include", "-Iruntime/contract/include",
          "-Iruntime/presentation/include", "-Iruntime/graphics/include", "-Iruntime/backends/dlss/include",
          "-Iui/overlay/include"],
         ["--no-recursive-allowlist", "--with-derive-default", "--raw-line", "#![allow(dead_code)]",
          "--raw-line", "use rsf_game_sys::sr_abi::rsf_overlay_sr_report;",
          "--raw-line", "use rsf_game_sys::{rsf_game_log_fn, rsf_result};"])
generate("games/anomaly/src/sr_swap_abi.rs", "games/anomaly/src/sr_swap_platform.h",
         "rsf_anomaly_swap_(fields|target|facts|spec|request|twins|upscale|counters|world_target|range|layout|platform|view)",
         "RSF_ANOMALY_SWAP_.*", [],
         ["--no-recursive-allowlist", "--with-derive-default", "--raw-line", "#![allow(dead_code)]"])
generate("games/anomaly/src/sr_stage_abi.rs", "games/anomaly/src/sr_stage_platform.h",
         "rsf_anomaly_sr_(log|settings|stage_(config|platform)|jitter_(state|sample|uv|sequence)|burst_(gate|verdict)|plan|frame_facts|controller|"
         "swap_counters|camera|pass(_inputs)?|planes|pose|reset_facts|seam|motion_target|temporal_textures|timer|choices)",
         "RSF_ANOMALY_SR_.*", [],
         ["--no-recursive-allowlist", "--with-derive-default", "--raw-line", "#![allow(dead_code)]",
          "--raw-line", "use rsf_game_sys::rsf_camera_frame;"])
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
         "rsf_fgvk(_(setup|frame|status|host_options|latency_status|provider|platform|frame_slot|timeline|submit_plan))?|rsf_dlss(gvk_status|_capture_source)",
         "RSF_FGVK_.*",
         ["-include", "runtime/orchestrator/src/native_fg_dlss_vulkan_capture_ledger.h",
          "-include", "runtime/orchestrator/src/vulkan_fg_frames.h",
          "-Iruntime/orchestrator/include", "-Iruntime/contract/include", "-Iruntime/graphics/include", "-Isdk/game/include"],
         ["--no-recursive-allowlist", "--raw-line", "use crate::vulkan_sr_abi::{rsf_srvk_image, rsf_srvk_setup};",
          "--raw-line", "use rsf_game_sys::rsf_frame_record;", "--raw-line", "type rsf_backend_result = i32;",
          "--with-derive-custom-struct", "rsf_dlss_capture_source=Copy,Clone"])
generate("runtime/orchestrator/src/vulkan_latency_abi.rs", "runtime/orchestrator/include/rescaleframe/native_latency_vulkan.h",
         "rsf_(latencyvk_(setup|status|device_query|device_requirements|created_device)|slvk_(setup|call|callback)|amd_vk_(update|platform))",
         "RSF_(LATENCYVK_(ABI_VERSION|DEVICE_ABI_VERSION)|SLVK_(ABI_VERSION|DLSS_SR|DLSS_FG))",
         ["-include", "rescaleframe/native_streamline_vulkan.h",
          "-include", "runtime/orchestrator/src/latency_amd_vulkan_platform.h", "-Iruntime/orchestrator/include",
          "-Iruntime/contract/include", "-Iruntime/graphics/include", "-Isdk/game/include"],
         ["--no-recursive-allowlist", "--raw-line", "type rsf_backend_result = i32;",
          "--raw-line", "use crate::vulkan_sr_abi::{rsf_vk_function, rsf_vk_get_proc};"])
generate("runtime/orchestrator/src/dx12_vulkan_interop_abi.rs", "runtime/orchestrator/src/native_dx12_vulkan_platform.h",
         "rsf_dx12vk_(platform|shared_image|interop_callbacks)", "RSF_DX12VK_INTEROP_.*",
         ["-Iruntime/orchestrator/include", "-Iruntime/contract/include", "-Iruntime/graphics/include", "-Isdk/game/include"],
         ["--no-recursive-allowlist", "--raw-line", "use crate::vulkan_sr_abi::rsf_srvk_setup;"])
generate("runtime/orchestrator/src/vulkan_fsr4_sr_abi.rs", "runtime/orchestrator/src/native_sr_fsr4_vulkan_platform.h",
         "rsf_fsr4vk_platform|rsf_fsr4_bridge_(create|frame|info)", "RSF_FSR4_BRIDGE_ABI_VERSION",
         ["-Iruntime/orchestrator/include", "-Iruntime/contract/include", "-Iruntime/graphics/include", "-Isdk/game/include"],
         ["--no-recursive-allowlist",
          "--raw-line", "use crate::dx12_vulkan_interop_abi::rsf_dx12vk_platform;",
          "--raw-line", "use crate::render_abi::rsf_sharpening_options;",
          "--raw-line", "use crate::vulkan_sr_abi::{rsf_fsr4vk_handoff, rsf_srvk_frame, rsf_srvk_image, rsf_srvk_setup};",
          "--raw-line", "use rsf_game_sys::rsf_frame_record;",
          "--raw-line", "type rsf_fsr4_bridge = ::core::ffi::c_void;"])
generate("runtime/orchestrator/src/vulkan_fg_resources_abi.rs", "runtime/orchestrator/src/vulkan_fg_resources.h",
         "rsf_vkfg_(platform|image|fences)", "NOTHING",
         ["-Isdk/game/include"], ["--no-recursive-allowlist", "--with-derive-default"])
generate("runtime/orchestrator/src/vulkan_dlss_sr_abi.rs", "runtime/orchestrator/src/native_dlss_vulkan_platform.h",
         "rsf_vkdlss_(platform|images)", "NOTHING",
         ["-Iruntime/orchestrator/include", "-Iruntime/contract/include", "-Iruntime/graphics/include", "-Isdk/game/include"],
         ["--no-recursive-allowlist", "--with-derive-default",
          "--raw-line", "use crate::vulkan_fg_resources_abi::rsf_vkfg_image;",
          "--raw-line", "use crate::vulkan_latency_abi::rsf_slvk_call;",
          "--raw-line", "use crate::vulkan_sr_abi::{rsf_srvk_frame, rsf_srvk_image, rsf_srvk_setup};"])
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
         "rsf_(dlss_pipeline_frame|native_translucency_masks)", "NOTHING",
         ["-include", "rescaleframe/native_translucency.h", "-Iruntime/orchestrator/include",
          "-Iruntime/backends/dlss/include", "-Iruntime/graphics/include", "-Iruntime/contract/include",
          "-Isdk/game/include"],
         ["--blocklist-type", "rsf_pipeline_camera_frame",
          "--raw-line", "use crate::render_abi::rsf_pipeline_camera_frame;",
          "--raw-line", "pub use rsf_game_sys::sr_abi::{RSF_DLSS_PIPELINE_ABI_VERSION, rsf_motion_decode_params};"])
# Match the overlay's existing std paths and import types that other modules already own.
generate("runtime/orchestrator/src/overlay_abi.rs", "runtime/orchestrator/src/overlay_platform.h",
         "wchar_t|rsf_overlay_(api|service_setup|service_option|capture|diagnostics|stats|intent|"
         "host_option|input_counters|platform|quality|texture_update)|rsf_native_(latency_status|input_routes)|rsf_render_status",
         "RSF_OVERLAY_(API_VERSION|CAPTURE_.*|NOTIFY_.*|SERVICE_OPTION_(TOGGLE|NUMBER|LIMIT)|"
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
          "--raw-line", "use rsf_game_sys::rsf_game_log_fn;",
          "--raw-line", "pub use rsf_game_sys::sr_abi::{\n"
                        "    RSF_OVERLAY_SR_INPUT_COLOR, RSF_OVERLAY_SR_INPUT_DEPTH, RSF_OVERLAY_SR_INPUT_EXPOSURE,\n"
                        "    RSF_OVERLAY_SR_INPUT_JITTER, RSF_OVERLAY_SR_INPUT_MOTION, rsf_overlay_sr_report,\n"
                        "};"],
         use_core=False)
generate("runtime/orchestrator/src/overlay_native12_abi.rs", "runtime/orchestrator/src/overlay_wrapped12_platform.h",
         "rsf_overlay(12_(log|platform|texture)|11on12_(device|target|platform))", "RSF_OVERLAY12_TEXTURE_LIMIT",
         ["-Iruntime/orchestrator/include", "-Iui/overlay/include", "-Isdk/game/include"],
         ["--no-recursive-allowlist", "--raw-line",
          "use crate::overlay_abi::{rsf_overlay_intent, rsf_overlay_stats, rsf_overlay_texture_update};"])
generate("runtime/orchestrator/src/vulkan_sr_abi.rs", "runtime/orchestrator/include/rescaleframe/native_sr_split_vulkan.h",
         "rsf_(srvk_(setup|frame|image|fallback|fsr4_provider_info)|fsr4vk_handoff|vk_(function|get_proc)|input_dump_vk_setup)",
         "RSF_(SRVK_(ABI_VERSION|SPLIT_ABI_VERSION|SPLIT_TRACKED_ABI_VERSION|FSR4_INFO_ABI_VERSION)|SR_(DLSS|FSR2|FSR3|FSR4|XESS)|FSR4_BRIDGE_(INVALID_ARGUMENT|ABI_MISMATCH))",
         ["-include", "runtime/vulkan/fsr4/bridge_api.h", "-include", "rescaleframe/input_dump_vulkan.h",
          "-Iruntime/orchestrator/include",
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
         "rsf_nfg_(platform|texture_info)",
         "RSF_NFG_(FORMAT_.*|STATE_.*|PHASE_.*|DUMP_VIEW_.*|SR_.*|FG_BACKEND_.*|STAT_.*|PRESENT_.*|RESULT_.*)",
         ["-Iruntime/orchestrator/include", "-Iruntime/contract/include", "-Iruntime/backends/dlss/include",
          "-Iruntime/graphics/include", "-Iruntime/presentation/include", "-Isdk/game/include"],
         ["--no-recursive-allowlist",
          "--raw-line", "use crate::native_fg_cpu_abi::rsf_nfg_cpu_platform;",
          "--raw-line", "use crate::render_abi::{\n"
                        "    rsf_backend_log_fn, rsf_dlss_frame, rsf_fg_frame, rsf_generation_provider,\n"
                        "};",
          "--raw-line", "use crate::scene_abi::rsf_native_scene_identity;",
          "--raw-line", "use rsf_game_sys::{rsf_frame_record, rsf_game_render_pass};"])
generate("runtime/orchestrator/src/native_fg_cpu_abi.rs", "runtime/orchestrator/src/native_fg_cpu_platform.h",
         "rsf_(nfg_(cpu_platform|receipt_candidate)|d3d11_present_status|streamline_latency_status|native_latency_window)",
         "RSF_(NFG_CPU_.*|PRESENT_STATUS_VERSION|SL_PROFILE_PCL|PACING_ANTILAG2)",
         ["-Iruntime/orchestrator/include", "-Iruntime/contract/include", "-Iruntime/backends/dlss/include",
          "-Iruntime/graphics/include", "-Iruntime/presentation/include", "-Isdk/game/include"],
         ["--no-recursive-allowlist",
          "--raw-line", "use crate::render_abi::{rsf_fg_status, rsf_pacing_owner, rsf_reflex_mode};",
          "--raw-line", "use rsf_game_sys::rsf_frame_record;"])

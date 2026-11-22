"""Regenerate the Windows x64 C ABI bindings using bindgen-cli 0.72.1."""
import os
import subprocess
from pathlib import Path
from tempfile import NamedTemporaryFile

root = Path(__file__).resolve().parents[1]
bindgen = os.environ.get("BINDGEN", "bindgen")
if subprocess.check_output([bindgen, "--version"], text=True).strip() != "bindgen 0.72.1":
    raise SystemExit("Install bindgen-cli 0.72.1 to regenerate the SDK bindings")

def generate(output, header, types, constants, clang_args, extra=(), *, use_core=True, derive_debug=False):
    bindings = subprocess.check_output([
        bindgen, header, "--allowlist-type", types, "--allowlist-var", constants,
        "--raw-line", "#![allow(non_camel_case_types)]",
        *([] if derive_debug else ["--no-derive-debug"]), *(["--use-core"] if use_core else []),
        "--no-doc-comments", "--rust-edition", "2024",
        "--rust-target", "1.85", *extra, "--", "--target=x86_64-pc-windows-msvc", *clang_args,
    ], cwd=root).replace(b"\r\n", b"\n")
    output = root / output
    if not output.exists() or output.read_bytes() != bindings:
        with NamedTemporaryFile(dir=output.parent, delete=False) as temporary:
            temporary.write(bindings)
        Path(temporary.name).replace(output)


generate("games/ac7/src/velocity_device_abi.rs", "games/ac7/src/velocity_device.h",
         "rsf_ac7_velocity_(variant|variant_request|kernels|refusal_fn)", "NOTHING", [])
generate("games/fallout4/src/resolution_stages.rs", "games/fallout4/src/resolution_stages.h",
         "NOTHING", "RSF_FALLOUT4_STAGE_.*", [])
generate("sdk/game/src/d3d11_reduced_abi.rs", "runtime/orchestrator/include/rescaleframe/d3d11_host.h",
         "rsf_d3d11_reduced_frame", "RSF_D3D11_REDUCED_.*",
         ["-Isdk/game/include", "-Iruntime/orchestrator/include", "-Iruntime/graphics/include",
          "-Iruntime/presentation/include"],
         ["--no-recursive-allowlist", "--with-derive-default",
          "--raw-line", "pub use crate::d3d11_host_abi::{\n"
          "    RSF_D3D11_AUXILIARY_COLOUR, RSF_D3D11_AUXILIARY_DEPTH_STENCIL,\n"
          "    RSF_D3D11_AUXILIARY_RESTORE_CONTENTS, rsf_d3d11_auxiliary_surface,\n};",
          "--raw-line", "use crate::rsf_game_render_pass;"])
generate("loader/src/diagnostics/frame_capture_abi.rs", "loader/diagnostics/src/frame_capture_platform.h",
         "rsf_capture_(result|calls|connect)", "NOTHING", ["-Iloader/diagnostics/include"])
generate("loader/src/diagnostics/memory_abi.rs", "loader/diagnostics/src/module_dump_memory.h",
         "rsf_mem_cache", "NOTHING", [], ["--with-derive-default"])
generate("loader/src/census/json_abi.rs", "loader/census/src/json_platform.h",
         "rsf_census_json", "NOTHING", [])
generate("loader/src/census/decode_abi.rs", "loader/census/src/decode_platform.h",
         "rsf_census_(resource_desc|read_resource)", "NOTHING", [],
         ["--with-derive-default"])
generate("tests/presentation/src/fixture_device_abi.rs", "tests/presentation/device_fixture_bridge.h",
         "rsf_fg_fixture_inputs", "NOTHING", [])
generate("tests/presentation/src/d3d11_host_device_abi.rs", "tests/d3d11_host_device_fixture.h",
         "rsf_d3d11_fixture_.*_fn", "RSF_D3D11_FIXTURE_.*",
         ["-Isdk/game/include", "-Iruntime/orchestrator/include", "-Iruntime/graphics/include",
          "-Iruntime/presentation/include"],
         ["--no-recursive-allowlist",
          "--allowlist-function", "rsf_d3d11_host_(reduced_frame|set_active|upscale_auxiliaries)",
          "--raw-line", "use rsf_game_sys::d3d11_host_abi::rsf_d3d11_auxiliary_pass;",
          "--raw-line", "use rsf_game_sys::d3d11_reduced_abi::rsf_d3d11_reduced_frame;"])
generate("tests/presentation/src/d3d11_marker_abi.rs", "tests/presentation/d3d11_marker_bridge.h",
         "rsf_(d3d11_marker_fixture_calls|native_cpu_(frame|counters)|native_latency_frame)",
         "NOTHING", ["-Isdk/game/include", "-Iruntime/orchestrator/include"],
         ["--no-recursive-allowlist",
          "--raw-line", "use rsf_game_sys::{rsf_game_cpu_event, rsf_game_render_pass};"])
generate("tests/presentation/api/src/abi.rs",
         "runtime/orchestrator/include/rescaleframe/d3d11_present_api.h",
         "rsf_(d3d11_present_(api|setup)|get_d3d11_present_api_fn)",
         "RSF_D3D11_PRESENT_API_(VERSION|ENTRY_POINT)",
         ["-Isdk/game/include", "-Iruntime/orchestrator/include", "-Iruntime/graphics/include",
          "-Iruntime/presentation/include", "-Iruntime/backends/dlss/include", "-Iruntime/contract/include"],
         ["--no-recursive-allowlist",
          "--raw-line", "use crate::{\n"
          "    rsf_backend_log_fn, rsf_generation_provider, rsf_latency_marker, rsf_observer_present_event_fn,\n"
          "    rsf_observer_present_fn, rsf_result, rsf_streamline_graphics, rsf_streamline_host,\n};"])
generate("tests/presentation/src/shared_surface_abi.rs",
         "runtime/presentation/include/rescaleframe/shared_surface.h",
         "rsf_shared_.*", "RSF_SHARED_SURFACE_ABI_VERSION", [],
         ["--allowlist-function", "rsf_shared_.*"])
generate("tests/loader/src/windows/vendor_census_asi_abi.rs",
         "loader/census/include/rescaleframe/vendor_census.h",
         "rsf_census_options", "RSF_CENSUS_(ABI_VERSION|FLAG_NO_STATIC_SCAN)", [], ["--with-derive-default"])
generate("loader/src/census/wrapper_abi.rs", "loader/census/src/wrapper_platform.h",
         "rsf_census_(vk_entry|call_storage|wrapper_services)", "NOTHING", [],
         ["--no-prepend-enum-name", "--blocklist-type", "rsf_census_json",
          "--raw-line", "#![allow(non_upper_case_globals)]",
          "--raw-line", "use super::json_abi::rsf_census_json;"])
generate("loader/src/census/hook_abi.rs", "loader/census/src/hook_platform.h",
         "rsf_census_(hook_(desc|platform)|visit_module)",
         "RSF_CENSUS_FLAG_(D3D12_DEBUG_ONLY|NO_(DXGI|VULKAN|GETPROCADDRESS_HOOK|LOADLIBRARY_HOOK))",
         ["-Iloader/census/include"])
generate("loader/src/census/record_abi.rs", "loader/census/src/record_platform.h",
         "rsf_census_(queued_record|record_writer)", "RSF_CENSUS_RECORD_(BODY_MAX|STACK_FRAMES)",
         ["-Iloader/census/include"],
         ["--blocklist-type", "rsf_census_hook_desc",
          "--raw-line", "use super::hook_abi::rsf_census_hook_desc;"])
generate("loader/src/census/stats_abi.rs", "loader/census/src/stats_platform.h",
         "rsf_census_(sample|stats_names)", "NOTHING", ["-Iloader/census/include"],
         ["--blocklist-type", "rsf_census_hook_desc",
          "--raw-line", "use super::hook_abi::rsf_census_hook_desc;"])
generate("loader/src/census/session_abi.rs", "loader/census/src/session_platform.h",
         "rsf_census_(call_state|core_platform|options|counters|result)",
         "RSF_CENSUS_(ABI_VERSION|FLAG_NO_STATIC_SCAN)", ["-Iloader/census/include"],
         ["--blocklist-type", "rsf_census_(hook_desc|json)",
          "--raw-line", "use super::hook_abi::rsf_census_hook_desc;",
          "--raw-line", "use super::json_abi::rsf_census_json;"])
generate("loader/src/census/ngx_abi.rs", "loader/census/src/ngx_platform.h",
         "rsf_census_ngx_platform", "RSF_CENSUS_NGX_ENTRIES", [],
         ["--blocklist-type", "rsf_census_json",
          "--raw-line", "use super::json_abi::rsf_census_json;"])
generate("loader/src/census/d3d_abi.rs", "loader/census/src/d3d_platform.h",
         "rsf_census_(d3d_(platform|entry)|debug_message)", "NOTHING", [],
         ["--no-prepend-enum-name", "--raw-line", "#![allow(non_upper_case_globals)]"])
generate("loader/src/census/misc_abi.rs", "loader/census/src/misc_platform.h",
         "rsf_census_misc_(platform|entry)", "NOTHING", [],
         ["--no-prepend-enum-name", "--blocklist-type", "rsf_census_json",
          "--raw-line", "#![allow(non_upper_case_globals)]",
          "--raw-line", "use super::json_abi::rsf_census_json;"])
generate("loader/src/census/fsr_abi.rs", "loader/census/src/fsr_platform.h",
         "rsf_census_fsr_(platform|entry)", "NOTHING", [],
         ["--no-prepend-enum-name", "--blocklist-type", "rsf_census_json",
          "--raw-line", "#![allow(non_upper_case_globals)]",
          "--raw-line", "use super::json_abi::rsf_census_json;"])
generate("tests/loader/src/windows/module_dump_abi.rs",
         "loader/diagnostics/include/rescaleframe/module_dump.h",
         "rsf_dump_(options|report|result)", "RSF_MODULE_DUMP_ABI_VERSION", [],
         ["--with-derive-default"])
generate("runtime/backends/fsr/compat/src/abi.rs", "runtime/backends/fsr/src/fsr4_compat_platform.h",
         "rsf_fsr4_compat_.*", "RSF_FSR4_COMPAT_PLATFORM_ABI_VERSION", [])
frame_types = "rsf_(frame_record|frame_id|frame_phase|screen_class|camera_frame)"
frame_values = "RSF_(GAME_(FRAME|CAMERA)_ABI_VERSION|FRAME_(FLAG|SOURCE)_.*)"
generate("sdk/game/src/frame_abi.rs", "sdk/game/include/rescaleframe/game_frame.h",
         frame_types, frame_values, [], ["--raw-line", "#![allow(dead_code)]"])
generate("runtime/vulkan/xess-fg/bridge_abi.rs", "runtime/vulkan/xess-fg/bridge_api.h",
         "rsf_xess_bridge_(create|start|frame|present_status)|VulkanStoryXessFrame", "RSF_XESS_BRIDGE_.*", [],
         ["--blocklist-type", "rsf_xess_bridge_legacy_reserved", "--raw-line", "#![allow(dead_code)]",
          "--raw-line", "type rsf_xess_bridge_legacy_reserved = ::core::mem::MaybeUninit<u32>;"])
generate("runtime/vulkan/xess-fg/session_records_abi.rs", "runtime/vulkan/xess-fg/session_records.h",
         "rsf_xess_(capture_desc|bridge_resource_desc|bridge_modules)",
         "RSF_XESS_BRIDGE_(PLATFORM_ABI_VERSION|ALLOCATOR_COUNT)", [],
         ["--with-derive-default", "--with-derive-custom-struct", "rsf_xess_bridge_resource_desc=PartialEq,Eq"])
generate("runtime/orchestrator/src/vulkan_xess_fg_abi.rs", "runtime/orchestrator/src/native_fg_xess_vulkan.h",
         "rsf_xessgvk_options", "NOTHING",
         ["-Iruntime/orchestrator/include", "-Isdk/game/include", "-Iruntime/contract/include"])
generate("runtime/orchestrator/src/vulkan_sr/xess_requirements_abi.rs",
         "runtime/orchestrator/include/rescaleframe/native_xess_vulkan_requirements.h",
         "rsf_xessvk_setup", "RSF_XESSVK_REQUIREMENTS_ABI_VERSION",
         ["-Iruntime/orchestrator/include", "-Isdk/game/include", "-Iruntime/contract/include"])
frame_records = "rsf_game_cpu_event"
frame_constants = (
    "RSF_(GAME_CPU_(FRAME_BEGIN|INPUT_SAMPLE|SIMULATION_BEGIN|"
    "SIMULATION_END|FRAME_END|INPUT_EVENT|EVENT_VERSION|INPUT_KEYED|INPUT_UNFRAMED)|"
    "GAME_RENDER_(SR|FINAL_SCENE|RESET|PRIMARY))"
)
generate("sdk/game/src/frame_event_abi.rs", "sdk/game/include/rescaleframe/game_api.h",
         frame_records, frame_constants, ["-Isdk/game/include"],
         ["--blocklist-type", frame_types])
generate("sdk/game/src/lib.rs", "sdk/game/include/rescaleframe/game_api.h",
         "rsf_(get_game_plugin_api_fn|frame_record|game_cpu_(source|receipt)_event)", "RSF_.*",
         ["-Isdk/game/include"], ["--blocklist-type", frame_records, "--blocklist-item", frame_constants,
          "--blocklist-type", frame_types, "--blocklist-item", frame_values,
          "--raw-line", "mod frame_abi;",
          "--raw-line", "pub use frame_abi::*;",
          "--raw-line", "mod frame_event_abi;",
          "--raw-line", "pub use frame_event_abi::*;",
          "--raw-line", "pub mod call_header;",
          "--raw-line", "pub mod callback_host_abi;",
          "--raw-line", "pub mod camera_math;",
          "--raw-line", "pub mod checksum;",
          "--raw-line", "pub mod d3d11_chain_abi;",
          "--raw-line", "pub mod d3d11_host_abi;",
          "--raw-line", "pub mod d3d11_reduced_abi;",
          "--raw-line", "pub mod d3d11_sr_admission_abi;",
          "--raw-line", "pub mod ffi_util;",
          "--raw-line", "pub mod plugin_session_abi;",
          "--raw-line", "pub mod presentation_result;",
          "--raw-line", "pub mod profile_text;",
          "--raw-line", "pub mod recording_slots;",
          "--raw-line", "mod render_config;",
          "--raw-line", "pub mod render_host_abi;",
          "--raw-line", "mod render_pass;",
          "--raw-line", "pub mod sampling;",
          "--raw-line", "pub mod session;",
          "--raw-line", "pub mod shader_patch_abi;",
          "--raw-line", "pub mod sharpening_abi;",
          "--raw-line", "pub mod sr_abi;",
          "--raw-line", "pub mod vendor_paths;",
          "--raw-line", "#[cfg(windows)]",
          "--raw-line", "pub mod win32;",
          "--raw-line", "pub mod windows_path;"])
generate("sdk/game/src/callback_host_abi.rs", "runtime/orchestrator/include/rescaleframe/callback_render_host.h",
         "rsf_callback_host_setup(_v2)?", "RSF_CALLBACK_HOST_ABI_VERSION(_V2)?",
         ["-Isdk/game/include", "-Iruntime/orchestrator/include"],
         ["--no-recursive-allowlist", "--raw-line", "type wchar_t = u16;",
          "--raw-line", "use crate::rsf_game_probe;"])
generate("sdk/game/src/sharpening_abi.rs",
         "runtime/orchestrator/include/rescaleframe/sharpening_settings.h",
         "rsf_sharpening_options", "NOTHING", [],
         ["--with-derive-custom-struct", "rsf_sharpening_options=Debug,Default,PartialEq"])
generate("sdk/game/src/shader_patch_abi.rs", "runtime/shader_patch/include/rescaleframe/shader_patch.h",
         "rsf_(get_shader_patch_api_fn|shader_patch_api|patch_.*)", "RSF_(SHADER_PATCH_VERSION|PATCH_.*)", [],
         ["--with-derive-default"])
generate("sdk/game/src/plugin_session_abi.rs", "runtime/orchestrator/include/rescaleframe/plugin_session.h",
         "rsf_plugin_session_options", "RSF_PLUGIN_SESSION_ABI_VERSION",
         ["-Isdk/game/include", "-Iruntime/orchestrator/include"],
         ["--no-recursive-allowlist", "--raw-line", "type wchar_t = u16;",
          "--raw-line", "use crate::{rsf_game_host_services, rsf_game_probe};"])
generate("games/project-wingman/src/overlay_abi.rs", "runtime/orchestrator/include/rescaleframe/overlay_api.h",
         "rsf_(start_d3d11_overlay_fn|adapter_setting_(read|write)_bool_fn)",
         "RSF_(OVERLAY_(API_VERSION|SERVICE_OPTION_TOGGLE)|RENDER_FRONTEND_ABI_VERSION|FG_BACKEND_AUTO|REFLEX_OFF)",
         ["-Isdk/game/include", "-Iruntime/orchestrator/include", "-Iruntime/contract/include",
          "-include", "rescaleframe/adapter_settings.h", "-include", "rescaleframe/frame_generation.h"])
generate("games/rdr2/src/asi/diagnostics_abi.rs", "games/rdr2/src/asi_platform.h",
         "rsf_(rdr2_asi_services|dump_(options|report)|census_(options|counters))",
         "RSF_(MODULE_DUMP|CENSUS)_ABI_VERSION",
         ["-Isdk/game/include", "-Iruntime/orchestrator/include", "-Iruntime/contract/include",
          "-Iloader/diagnostics/include", "-Iloader/census/include"],
         ["--with-derive-default"])
generate("games/rdr2/src/runtime_worker_abi.rs", "games/rdr2/src/runtime_setup.h",
         "rsf_(rdr2_runtime_leaves|rdr2_fingerprint_fn|render_host_setup)",
         "RSF_(RENDER_HOST_(ABI_VERSION|OWNS_PRESENTATION)|UI_MODE_BACKBUFFER_HUDLESS)",
         ["-Isdk/game/include", "-Iruntime/orchestrator/include", "-Iruntime/contract/include"],
         ["--no-recursive-allowlist", "--raw-line", "type wchar_t = u16;"])
generate("games/rdr2/src/script_input_abi.rs", "runtime/orchestrator/include/rescaleframe/render_frontend.h",
         "rsf_render_settings", "RSF_RENDER_FRONTEND_ABI_VERSION", ["-Iruntime/orchestrator/include"],
         ["--no-recursive-allowlist"])
generate("ui/overlay/src/host_option_limit.rs", "runtime/orchestrator/include/rescaleframe/overlay_api.h",
         "NOTHING", "RSF_OVERLAY_SERVICE_OPTION_LIMIT",
         ["-Isdk/game/include", "-Iruntime/orchestrator/include", "-Iruntime/contract/include",
          "-Iruntime/presentation/include", "-Iruntime/graphics/include", "-Iruntime/backends/dlss/include"])
generate("sdk/game/src/sharpening_defaults.rs", "runtime/orchestrator/include/rescaleframe/sharpening_settings.h",
         "NOTHING", "RSF_SHARPENING_DEFAULT_.*", [])
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
generate("runtime/orchestrator/src/dlss_streamline_abi.rs", "runtime/backends/dlss/include/rescaleframe/dlss.h",
         "rsf_dlss_(plan|support|quality)", "NOTHING", ["-Iruntime/backends/dlss/include"],
         ["--no-recursive-allowlist"])
generate("runtime/orchestrator/src/resource_roles_abi.rs", "runtime/graphics/include/rescaleframe/resource_roles.h",
         "rsf_(texture_facts|frame_shape|resource_role|role_(table|rule|verdict|confidence))",
         "RSF_ROLE_RULE_.*", [], ["--no-recursive-allowlist"])
generate("runtime/orchestrator/src/scene_promote_abi.rs", "runtime/graphics/include/rescaleframe/scene_promote.h",
         "rsf_promote_(setup|frame_tail|status|log_fn)",
         "RSF_PROMOTE_(ABI_VERSION|MAX_(UI|CHAIN)_TARGETS)",
         ["-Iruntime/graphics/include"], ["--no-recursive-allowlist", "--with-derive-default"])
generate("sdk/game/src/d3d11_sr_admission_abi.rs", "runtime/orchestrator/include/rescaleframe/gpu_policy.h",
         "rsf_d3d11_sr_admission", "RSF_(D3D11_SR_ADMISSION_ABI_VERSION|SR_AUTO)",
         ["-include", "rescaleframe/sr_session.h", "-Iruntime/orchestrator/include",
          "-Iruntime/contract/include", "-Isdk/game/include"],
         ["--no-recursive-allowlist", "--with-derive-default"])
generate("sdk/game/src/d3d11_chain_abi.rs", "runtime/orchestrator/include/rescaleframe/d3d11_host.h",
         "rsf_d3d11_chain_dispatch.*", "RSF_D3D11_(HOST_(ABI_VERSION|DEPTH_.*)|DISPATCH_.*)",
         ["-Isdk/game/include", "-Iruntime/orchestrator/include", "-Iruntime/graphics/include",
          "-Iruntime/presentation/include"],
         ["--no-recursive-allowlist", "--raw-line", "#![allow(dead_code)]"])
generate("games/fallout4/src/platform_abi.rs", "games/fallout4/src/fallout4_platform.h",
         "rsf_(fallout4_(.*_fn|create_call)|d3d11_presentation_binding)",
         "RSF_(FALLOUT4_.*|D3D11_HOST_(ABI_VERSION|CAP_.*|DEPTH_.*))",
         ["-Isdk/game/include", "-Iruntime/orchestrator/include", "-Iruntime/graphics/include",
          "-Iruntime/presentation/include"],
         ["--no-recursive-allowlist", "--raw-line", "#![allow(dead_code)]", "--raw-line", "type wchar_t = u16;",
          "--raw-line", "pub use rsf_game_sys::d3d11_host_abi::{\n"
          "    rsf_d3d11_creation_scope, rsf_d3d11_host_setup, rsf_d3d11_native_chain,\n};",
          "--raw-line", "use rsf_game_sys::rsf_game_host_services;",
          "--raw-line", "pub use rsf_game_sys::sr_abi::rsf_motion_decode_params;"])
generate("games/fallout4/src/cpu_abi.rs", "games/fallout4/src/cpu_events_platform.h",
         "rsf_fallout4_cpu_(identity|scope)", "RSF_FALLOUT4_CPU_(HOOK_.*|SCOPE_WORDS)",
         ["-Isdk/game/include"],
         ["--no-recursive-allowlist", "--raw-line", "#![allow(dead_code)]",
          "--with-derive-custom-struct", "rsf_fallout4_cpu_identity=Debug,PartialEq,Eq"])
generate("games/ac7/src/platform_abi.rs", "games/ac7/src/ac7_platform.h",
         "rsf_ac7_(platform|native_renderer(_options)?|renderer_log_fn|render_scope(_fn)?)", "RSF_AC7_DIAG_.*",
         ["-Isdk/game/include", "-Igames/ac7/include"],
         ["--no-recursive-allowlist", "--raw-line", "#![allow(dead_code)]", "--raw-line",
          "use rsf_game_sys::{rsf_game_cpu_event_fn, rsf_game_render_config_fn, rsf_game_render_pass};",
          "--with-derive-custom-struct", "rsf_ac7_native_renderer_options=Copy,Clone"])
generate("games/ac7/src/sites_abi.rs", "games/ac7/src/ac7_sites.h",
         "rsf_ac7_(code_(leaves|patch|patches)|renderer_leaves|call_relay|truesky_(patches|leaves))",
         "RSF_AC7_(ROLE_.*|HOOK_SITE_COUNT|PROCESS_HOOKS|EVIDENCE_(FIRST|LAST)|SITE_BYTES|VELOCITY_PATCHES|GUARD_.*|"
         "RELAY_BYTES)",
         ["-Isdk/game/include", "-Igames/ac7/include"],
         ["--no-recursive-allowlist", "--raw-line", "#![allow(dead_code)]",
          "--raw-line", "use crate::platform_abi::{\n"
          "    rsf_ac7_native_renderer, rsf_ac7_native_renderer_options, rsf_ac7_renderer_log_fn,\n};"])
generate("games/ac7/src/scope_abi.rs", "games/ac7/src/ac7_scope_owner.h",
         "rsf_ac7_scope_(lease|calls|read)", "NOTHING",
         ["-Isdk/game/include", "-Igames/ac7/include"],
         ["--no-recursive-allowlist", "--raw-line", "#![allow(dead_code)]",
          "--raw-line", "use crate::platform_abi::{rsf_ac7_render_scope, rsf_ac7_render_scope_fn};"])
generate("games/ac7/src/view_abi.rs", "games/ac7/include/rescaleframe/ac7_view.h",
         "rsf_ac7_view(_result)?", "RSF_AC7_VIEW_.*", [],
         ["--with-derive-default", "--raw-line", "#![allow(dead_code)]"])
generate("games/ac7/src/ui_abi.rs", "games/ac7/include/rescaleframe/ac7_ui_rules.h",
         "rsf_ac7_(draw_(class|facts|input)|layout_(kind|element)|texture_facts|ui_registry)",
         "RSF_AC7_(BLEND|FORMAT|UI|DRAW|LAYOUT)_.*", [],
         ["--raw-line", "#![allow(dead_code)]"])
generate("games/ac7/src/scene_color_abi.rs", "games/ac7/include/rescaleframe/ac7_scene_color.h",
         "rsf_(ac7_scene_color|frame_tap_(input|target_draw|geometry))", "RSF_FRAME_TAP_MAX_INPUTS",
         ["-Iruntime/graphics/include"],
         ["--no-recursive-allowlist", "--with-derive-default", "--raw-line", "#![allow(dead_code)]"])
generate("games/ac7/src/cloud_depth_abi.rs", "games/ac7/src/cloud_depth_platform.h",
         "rsf_ac7_cloud_depth", "NOTHING", [],
         ["--with-derive-default", "--raw-line", "#![allow(dead_code)]"])
generate("games/minecraft/native/platform_abi.rs", "runtime/orchestrator/include/rescaleframe/gl_pipeline.h",
         "rsf_(gl_pipeline(_(setup|options|surfaces|status))?|"
         "native_fg_(options|status)|fg_status|sharpening_options|native_(latency_status|cpu_counters)|"
         "backend_(log_fn|result)|dump_log_fn|fg_mode|pacing_owner|reflex_mode)",
         "RSF_(SHARPENING_ABI_VERSION|FG_(OFF|FIXED))",
         ["-include", "rescaleframe/input_dump.h", "-include", "rescaleframe/native_cpu.h",
          "-Iruntime/orchestrator/include", "-Iruntime/backends/dlss/include", "-Iruntime/graphics/include",
          "-Iruntime/contract/include", "-Iruntime/presentation/include", "-Isdk/game/include"],
         ["--no-recursive-allowlist", "--with-derive-default", "--raw-line", "#![allow(dead_code)]",
          "--allowlist-function",
          "rsf_(gl_window_(create|destroy)|gl_pipeline_(open|configure(_legacy|_settings)?|settings_(open|revision)|get_surfaces|cpu|input|abort|"
          "evaluate|hudless|present|status_get|close)|input_dump_configure|native_latency_status_get)",
          "--raw-line", "use rsf_game_sys::rsf_game_render_pass;",
          "--raw-line", "type wchar_t = u16;"])
generate("games/anomaly/src/fg_stage/fixture_cpu_abi.rs", "games/anomaly/src/fg_stage/fixture_platform.h",
         "rsf_(native_cpu_(frame|counters)|anomaly_fg_fixture_platform)", "NOTHING",
         ["-Isdk/game/include", "-Iruntime/orchestrator/include", "-Iruntime/graphics/include",
          "-Iruntime/contract/include", "-Iruntime/presentation/include", "-Iruntime/backends/dlss/include"],
         ["--no-recursive-allowlist", "--with-derive-default",
          "--raw-line", "use crate::fg_stage_abi::rsf_anomaly_fg_state_scope;"])
generate("games/anomaly/src/motion_frame_target_abi.rs", "games/anomaly/src/motion_frame_target.h",
         "rsf_anomaly_motion_frame_target", "NOTHING", [], ["--with-derive-default"])
generate("games/anomaly/src/shader_loader/frame_abi.rs", "games/anomaly/src/shader_frame_layout.h",
         "rsf_anomaly_shader_frame_layout", "NOTHING", [])
generate("games/anomaly/src/fg_stage_abi.rs", "games/anomaly/src/fg_stage_platform.h",
         "rsf_(anomaly_fg_.*|d3d11_(present_(api|setup)|fg_api|state)|get_d3d11_(present|fg)_api_fn|"
         "native_fg_(options|status)|fg_status|d3d11_fg_inputs|observer_present(_event)?(_fn)?|backend_log_fn)",
         "RSF_(D3D11_(PRESENT|FG)_API_(VERSION|ENTRY_POINT)|D3D11_FG_INPUTS_VERSION|"
         "FG_(BACKEND_.*|OFF|FIXED|STAT_TOTAL_PRESENTED)|REFLEX_(ON|BOOST)|UI_MODE_BACKBUFFER_HUDLESS)",
         ["-include", "rescaleframe/d3d11_state.h", "-Isdk/game/include", "-Iruntime/orchestrator/include", "-Iruntime/contract/include",
          "-Iruntime/graphics/include", "-Iruntime/presentation/include", "-Iruntime/backends/dlss/include"],
         ["--no-recursive-allowlist", "--with-derive-default", "--raw-line", "#![allow(dead_code)]",
          "--raw-line", "use crate::motion_frame_target_abi::rsf_anomaly_motion_frame_target;",
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
generate("games/anomaly/src/bootstrap_abi.rs", "games/anomaly/src/bootstrap_platform.h",
         "wchar_t|rsf_anomaly_(build_identity|bootstrap_platform)", "NOTHING", ["-Isdk/game/include"],
         ["--no-recursive-allowlist", "--raw-line", "use rsf_game_sys::rsf_game_log_fn;"])
generate("games/anomaly/src/native_observer_abi.rs", "games/anomaly/src/native_observer_platform.h",
         "rsf_anomaly_(native_observation|observer_(profile|binding|engine|frame|camera|platform))",
         "RSF_ANOMALY_(OBSERVATION_ABI|OBSERVER_HOOK_COUNT)", ["-Isdk/game/include"],
         ["--no-recursive-allowlist", "--with-derive-default",
          "--raw-line", "use rsf_game_sys::rsf_game_log_fn;"])
generate("games/anomaly/src/overlay_abi.rs", "games/anomaly/src/overlay_platform.h",
         "wchar_t|rsf_anomaly_overlay_(option|chain|options)|"
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
         "rsf_anomaly_sr_(log|settings|stage_(config|platform)|driver_config|jitter_(state|sample|uv|sequence)|burst_(gate|verdict)|plan|frame_reads|controller|"
         "camera|pass(_inputs)?|reset_facts|motion_target|timer|choices)",
         "RSF_ANOMALY_SR_.*", [],
         ["--no-recursive-allowlist", "--with-derive-default", "--raw-line", "#![allow(dead_code)]",
          "--raw-line", "use crate::sr_d3d11::DrawViews as rsf_sr_draw_views;",
          "--with-derive-custom-struct", "rsf_anomaly_sr_settings=PartialEq,Eq",
          "--raw-line", "pub use crate::sr_swap_abi::rsf_anomaly_swap_counters as rsf_anomaly_sr_swap_counters;",
          "--raw-line", "pub use crate::sr_swap_abi::rsf_anomaly_swap_facts as rsf_anomaly_sr_frame_facts;",
          "--raw-line", "use rsf_game_sys::rsf_camera_frame;"])
generate("games/unity-mono/src/plugin_abi.rs", "games/unity-mono/include/rescaleframe/unity_bridge.h",
         "rsf_unity_native_api", "NOTHING",
         ["-Isdk/game/include", "-Igames/unity-mono/include"],
         ["--no-recursive-allowlist", "--raw-line", "#![allow(dead_code)]",
          "--raw-line", "use rsf_game_sys::rsf_game_cpu_event;",
          "--raw-line", "type rsf_unity_config = ::core::ffi::c_void;",
          "--raw-line", "type rsf_unity_packet = ::core::ffi::c_void;"])
generate("games/unity-mono/src/bridge_abi.rs", "games/unity-mono/src/bridge_state.h",
         "rsf_unity_(packet|config|frame_admission|source_submission|texture_ops)",
         "RSF_UNITY_(BRIDGE_ABI_VERSION|PACKET_.*|FRAMES_.*|PREVIEW_SOURCE_COMMON)",
         ["-Isdk/game/include", "-Igames/unity-mono/include"],
         ["--no-recursive-allowlist", "--raw-line", "#![allow(dead_code)]",
          "--raw-line", "use rsf_game_sys::rsf_camera_frame;",
          "--with-derive-custom-struct", "rsf_unity_packet=Copy,Clone"])
generate("games/unity-mono/src/bridge_d3d12_abi.rs", "games/unity-mono/src/bridge_d3d12.h",
         "rsf_unity_d3d12_engine", "NOTHING", ["-Isdk/game/include", "-Igames/unity-mono/include"])
generate("games/unity-mono/src/shared_surface_abi.rs", "runtime/presentation/include/rescaleframe/shared_surface.h",
         "rsf_shared_.*", "RSF_SHARED_SURFACE_ABI_VERSION", [],
         ["--allowlist-function",
          "rsf_shared_(surface_(create|destroy|d3d11|d3d12)|fence_(create|destroy|d3d11|d3d12))"])
generate("games/unity-mono/src/vulkan_handles.rs", "games/unity-mono/src/vulkan_state.h",
         "rsf_unity_vulkan_(device|command|image)_handles", "NOTHING",
         ["-Igames/unity-mono/include", "-Isdk/game/include"],
         ["--no-recursive-allowlist"])
generate("games/unity-mono/src/runtime_abi.rs", "runtime/orchestrator/include/rescaleframe/native_fg_d3d12.h",
         "rsf_(fg12_setup|fg_choice|native_fg_options|backend_result|backend_log_fn)|wchar_t",
         "RSF_(FG_BACKEND_(DLSS|FSR3|FSR4|XESS|AUTO|ALL)|FG_FIXED|REFLEX_OFF|UI_MODE_BACKBUFFER_HUDLESS|"
         "SR_(DLSS|FSR2|FSR3|FSR4|XESS|SPATIAL|AUTO))",
         ["-include", "rescaleframe/sr_session.h", "-include", "rescaleframe/fg_choice.h", "-Iruntime/orchestrator/include", "-Iruntime/contract/include",
          "-Iruntime/backends/dlss/include", "-Iruntime/graphics/include", "-Iruntime/presentation/include",
          "-Isdk/game/include"],
         ["--no-recursive-allowlist", "--raw-line", "#![allow(dead_code)]"])
generate("games/unity-mono/src/preview_abi.rs", "runtime/orchestrator/include/rescaleframe/d3d12_window_presenter.h",
         "rsf_d3d12_window_(presenter|target|observer)", "RSF_D3D12_WINDOW_TARGET_ABI_VERSION",
         ["-Isdk/game/include"],
         ["--no-recursive-allowlist", "--allowlist-function", "rsf_d3d12_window_.*",
          "--raw-line", "use rsf_game_sys::rsf_game_render_pass;"])
generate("games/unity-mono/src/package_abi.rs", "integrations/unity/native/package_state.h",
         "rsf_(unity_(package_(settings|status|platform|view_setup)|frame_counters)|native_fg_(options|status)|fg_status|"
         "native_(latency_status|cpu_counters)|fg_mode|pacing_owner|reflex_mode|sr12(_setup|_plan_result)?|dlss_(setup|engine|log_fn))",
         "RSF_(UNITY_(SETTINGS|PACKAGE)_ABI_VERSION|DLSS_ABI_VERSION|SR12_PLAN_ABI_VERSION|SR_NONE|FG_(OFF|STAT_TOTAL_PRESENTED))",
         ["-include", "rescaleframe/native_sr_d3d12.h", "-include", "rescaleframe/native_fg.h", "-include", "rescaleframe/native_cpu.h",
          "-include", "rescaleframe/sr_session.h", "-Iruntime/orchestrator/include", "-Iruntime/contract/include",
          "-Iruntime/backends/dlss/include", "-Iruntime/graphics/include", "-Iruntime/presentation/include",
          "-Isdk/game/include", "-Igames/unity-mono/include"],
         ["--no-recursive-allowlist", "--with-derive-default", "--raw-line", "#![allow(dead_code)]",
          "--raw-line", "type wchar_t = u16;\ntype rsf_backend_result = i32;",
          "--raw-line", "use crate::plugin_abi::rsf_unity_native_api;",
          "--raw-line", "use crate::runtime_abi::{rsf_backend_log_fn, rsf_fg12_setup};",
          "--raw-line", "pub use rsf_game_sys::sharpening_abi::rsf_sharpening_options;",
          "--raw-line", "use rsf_game_sys::{rsf_game_cpu_event, rsf_game_host_services, rsf_game_render_pass};",
          "--with-derive-custom-struct", "rsf_unity_package_platform=Copy,Clone"])
generate("tests/engine_sharpening_abi.rs", "integrations/unity/native/package_state.h",
         "rsf_(godot_settings|unity_package_settings|srvk_(frame|image))",
         "RSF_(GODOT_ABI_VERSION|GODOT_SETTINGS_ABI_VERSION|UNITY_SETTINGS_ABI_VERSION|SRVK_ABI_VERSION)",
         ["-include", "rescaleframe/native_fg.h", "-include", "rescaleframe/native_cpu.h",
          "-include", "rescaleframe/sr_session.h", "-include", "integrations/godot/native/api.h",
          "-Iruntime/orchestrator/include",
          "-Iruntime/contract/include", "-Iruntime/backends/dlss/include", "-Iruntime/graphics/include",
          "-Iruntime/presentation/include", "-Isdk/game/include", "-Igames/unity-mono/include"],
         ["--no-recursive-allowlist", "--with-derive-default",
          "--raw-line", "use crate::sharpening_abi::rsf_sharpening_options;",
          "--raw-line", "type rsf_frame_record = ::core::ffi::c_void;",
          "--with-derive-custom-struct", "rsf_(godot_settings|unity_package_settings)=PartialEq"])
generate("runtime/orchestrator/src/d3d12_sr/legacy_abi.rs", "runtime/contract/include/rescaleframe/backend.h",
         "rsf_(fg_provider|fg_swapchain_result)", "NOTHING",
         ["-Isdk/game/include"],
         ["--no-recursive-allowlist", "--raw-line",
          "use crate::render_abi::{\n"
          "    rsf_backend_caps, rsf_backend_probe_desc, rsf_backend_result, rsf_fg_frame,\n"
          "    rsf_fg_swapchain_desc, rsf_latency_marker, rsf_ui_mode,\n};"])
generate("runtime/orchestrator/src/render_abi.rs", "runtime/orchestrator/include/rescaleframe/frame_assembly.h",
         "rsf_(frame_resources|pipeline_camera_frame|dlss_frame|fg_choice|render_link|sr_session_setup|sr_session_status|sr_provider|fg_host|fg_session_status|generation_provider|native_fg_status|motion_resolve_params|render_settings)",
         "RSF_(FRAME_ASSEMBLY_ABI_VERSION|DLSS_ABI_VERSION|FG_(BACKEND_(AUTO|ALL)|ABI_VERSION|OFF|FIXED|AUTO|DYNAMIC|STAT_(TOTAL_PRESENTED|GENERATED_PRESENTED|PRESENT_CALLBACKS|ACTIVITY))|NATIVE_FG_REASON_NONE|PACING_(NONE|REFLEX|XELL)|REFLEX_(OFF|BOOST)|RENDER_LINK_ABI_VERSION|SR_SESSION_ABI_VERSION|BACKEND_ABI_VERSION|API_D3D12|SR_(FSR2|XESS)|RENDER_FRONTEND_ABI_VERSION|SHARPENING_ABI_VERSION)",
         ["-include", "rescaleframe/fg_choice.h", "-include", "rescaleframe/render_links.h",
          "-include", "rescaleframe/render_frontend.h",
          "-include", "rescaleframe/sr_session.h", "-include", "rescaleframe/fg_session.h",
          "-include", "rescaleframe/native_fg.h", "-include", "rescaleframe/motion_resolve.h", "-Iruntime/orchestrator/include",
          "-Iruntime/contract/include", "-Iruntime/backends/dlss/include", "-Iruntime/graphics/include",
          "-Iruntime/presentation/include", "-Isdk/game/include"],
         ["--blocklist-type", "rsf_(frame_record|frame_id|frame_phase|screen_class|camera_frame|sharpening_options)",
          "--raw-line", "use rsf_game_sys::rsf_frame_record;",
          "--raw-line", "pub use rsf_game_sys::sharpening_abi::rsf_sharpening_options;"])
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
         "rsf_(latencyvk_(setup|status|device_query|device_requirements|created_device)|slvk_(setup|call|callback))",
         "RSF_(LATENCYVK_(ABI_VERSION|DEVICE_ABI_VERSION)|SLVK_(ABI_VERSION|DLSS_SR|DLSS_FG))",
         ["-include", "rescaleframe/native_streamline_vulkan.h",
          "-include", "rescaleframe/native_latency_vulkan_requirements.h", "-Iruntime/orchestrator/include",
          "-Iruntime/contract/include", "-Iruntime/graphics/include", "-Isdk/game/include"],
         ["--no-recursive-allowlist", "--raw-line", "type rsf_backend_result = i32;",
          "--raw-line", "use crate::vulkan_sr_abi::{rsf_vk_function, rsf_vk_get_proc};"])
generate("tests/game-sdk/src/vk_latency_calls.rs", "runtime/orchestrator/src/native_latency_vulkan_presentation.h",
         "rsf_(latencyvk_(setup|status|device_query|device_requirements|created_device)|"
         "slvk_(setup|call|callback)|fgvk(_latency_status)?|vk_(function|get_proc|wsi_call))",
         "RSF_(LATENCYVK_.*|SLVK_.*|FGVK_LATENCY_ABI_VERSION|FG_BACKEND_XESS|PACING_(XELL|REFLEX|ANTILAG_VULKAN))",
         ["-include", "rescaleframe/native_streamline_vulkan.h",
          "-include", "rescaleframe/native_latency_vulkan.h", "-include", "rescaleframe/native_latency_vulkan_requirements.h",
          "-include", "rescaleframe/native_cpu.h", "-include", "rescaleframe/frame_generation.h", "-Iruntime/orchestrator/include",
          "-Iruntime/contract/include", "-Isdk/game/include"],
         ["--no-recursive-allowlist", "--with-derive-default",
          "--allowlist-function", "rsf_(latencyvk_.*|slvk_.*|native_latency_(claim|has_claim|release_claim)|core_fgvk_presentation_(attach|borrow|return))",
          "--raw-line", "type rsf_backend_result = i32;",
          "--raw-line", "use rsf_game_sys::{rsf_game_cpu_event, rsf_game_render_pass};"])

generate("runtime/orchestrator/src/vulkan_latency_platform_abi.rs", "runtime/orchestrator/src/native_latency_vulkan_platform.h",
         "rsf_vk_(latency_platform|streamline_calls|token_calls|pcl_calls)", "NOTHING",
         ["-Iruntime/orchestrator/include", "-Iruntime/contract/include", "-Isdk/game/include"],
         ["--no-recursive-allowlist",
          "--raw-line", "use crate::vulkan_fg_abi::{rsf_fgvk, rsf_fgvk_latency_status};",
          "--raw-line", "use crate::vulkan_latency_abi::rsf_slvk_call;",
          "--raw-line", "type rsf_backend_result = i32;"])
generate("runtime/orchestrator/src/dx12_vulkan_interop_abi.rs", "runtime/orchestrator/src/native_dx12_vulkan_platform.h",
         "rsf_dx12vk_(platform|shared_image|interop_callbacks)", "RSF_DX12VK_INTEROP_.*",
         ["-Iruntime/orchestrator/include", "-Iruntime/contract/include", "-Iruntime/graphics/include", "-Isdk/game/include"],
         ["--no-recursive-allowlist", "--raw-line", "use crate::vulkan_sr_abi::rsf_srvk_setup;"])
generate("runtime/vulkan/fsr4/bridge_abi.rs", "runtime/vulkan/fsr4/bridge_api.h",
         "rsf_fsr4_bridge_(create|frame|info)", "RSF_FSR4_BRIDGE_.*", [],
         ["--no-recursive-allowlist"])
generate("runtime/vulkan/fsr4-fg/bridge_abi.rs", "runtime/vulkan/fsr4-fg/bridge_api.h",
         "rsf_fsr4fg_(create|frame|info|present_status)", "RSF_FSR4FG_.*", ["-Isdk/game/include"],
         ["--no-recursive-allowlist", "--raw-line", "use rsf_game_sys::rsf_frame_record;"])
generate("runtime/orchestrator/src/vulkan_fsr4_sr_abi.rs", "runtime/orchestrator/src/native_sr_fsr4_vulkan_platform.h",
         "rsf_fsr4vk_platform", "NOTHING",
         ["-Iruntime/orchestrator/include", "-Iruntime/contract/include", "-Iruntime/graphics/include", "-Isdk/game/include"],
         ["--no-recursive-allowlist",
          "--raw-line", "use super::bridge_abi::{rsf_fsr4_bridge_create, rsf_fsr4_bridge_frame, rsf_fsr4_bridge_info};",
          "--raw-line", "use crate::dx12_vulkan_interop_abi::rsf_dx12vk_platform;",
          "--raw-line", "use crate::render_abi::rsf_sharpening_options;",
          "--raw-line", "use crate::vulkan_fg_resources_abi::rsf_vkfg_barrier;",
          "--raw-line", "use crate::vulkan_sr_abi::{rsf_fsr4vk_handoff, rsf_srvk_frame, rsf_srvk_image, rsf_srvk_setup};",
          "--raw-line", "use rsf_game_sys::rsf_frame_record;",
          "--raw-line", "type rsf_fsr4_bridge = ::core::ffi::c_void;"])
generate("runtime/orchestrator/src/vulkan_fg_resources_abi.rs", "runtime/orchestrator/src/vulkan_fg_resources.h",
         "rsf_vkfg_(platform|image|fences|barrier|image_copy)", "NOTHING",
         ["-Isdk/game/include", "-include", "runtime/orchestrator/src/vulkan_fg_copy_abi.h"],
         ["--no-recursive-allowlist", "--with-derive-default",
          "--with-derive-custom-struct", "rsf_vkfg_barrier=Debug,PartialEq,Eq",
          "--with-derive-custom-struct", "rsf_vkfg_image_copy=Debug,PartialEq,Eq"])
generate("runtime/orchestrator/src/d3d11_sharpening_abi.rs", "runtime/graphics/src/sharpening/d3d11.h",
         "rsf_sharpen11_frame", "NOTHING", ["-Iruntime/orchestrator/include"],
         ["--no-recursive-allowlist"])
generate("runtime/orchestrator/src/vulkan_sharpening_abi.rs", "runtime/orchestrator/src/sharpening_vulkan.h",
         "rsf_sharpenvk_(uniform|layouts|facts|pass|platform|host)", "NOTHING",
         ["-Iruntime/orchestrator/include", "-Iruntime/contract/include", "-Iruntime/graphics/include", "-Isdk/game/include"],
         ["--no-recursive-allowlist", "--with-derive-default",
          "--blocklist-type", "rsf_(sharpening_options|vkfg_platform|srvk_(setup|frame))",
          "--raw-line", "type rsf_sharpenvk = ::core::ffi::c_void;",
          "--raw-line", "use crate::render_abi::rsf_sharpening_options;",
          "--raw-line", "use crate::vulkan_fg_resources_abi::rsf_vkfg_platform;",
          "--raw-line", "use crate::vulkan_sr_abi::{rsf_srvk_frame, rsf_srvk_setup};"])
generate("runtime/vulkan/streamline/lifecycle/src/sdk_fg_options_abi.rs", "runtime/vulkan/streamline/sdk_fg_options.h",
         "rsf_slvk_sdk_fg_options", "NOTHING", [], ["--with-derive-default"])
generate("runtime/vulkan/streamline/lifecycle/src/sdk_records_abi.rs", "runtime/vulkan/streamline/sdk_records.h",
         "rsf_slvk_(sdk_.*|legacy_.*)", "NOTHING", [], ["--with-derive-default"])
generate("runtime/orchestrator/src/vulkan_dlss_sr_abi.rs", "runtime/orchestrator/src/native_dlss_vulkan_platform.h",
         "rsf_vkdlss_(platform|images|commands)", "NOTHING",
         ["-Iruntime/orchestrator/include", "-Iruntime/contract/include", "-Iruntime/graphics/include", "-Isdk/game/include"],
         ["--no-recursive-allowlist", "--with-derive-default",
          "--raw-line", "use crate::vulkan_fg_resources_abi::rsf_vkfg_image;",
          "--raw-line", "use crate::vulkan_latency_abi::rsf_slvk_call;",
          "--raw-line", "use crate::vulkan_sr_abi::{rsf_srvk_frame, rsf_srvk_image, rsf_srvk_setup, rsf_vk_function};",
          "--raw-line", "use rsf_game_sys::rsf_frame_record;"])
generate("runtime/orchestrator/src/cpu_abi.rs", "runtime/orchestrator/include/rescaleframe/native_cpu.h",
         "rsf_native_(cpu_(frame|counters|reservation|reservation_counters)|input_receipts)",
         "RSF_NATIVE_CPU_(RESERVATION_.*|DEPENDENCY_.*|PRESENT_.*)",
         ["-Iruntime/orchestrator/include", "-Isdk/game/include"])
generate("tests/game-sdk/src/antilag_abi.rs", "runtime/orchestrator/src/latency_antilag.h",
         "AntiLag(Callbacks|Context|State)", "NOTHING", [],
         ["--allowlist-function", "rsf_core_antilag_.*", "--with-derive-default"])
generate("tests/game-sdk/src/cpu_calls.rs", "runtime/orchestrator/include/rescaleframe/native_cpu.h",
         "NOTHING", "NOTHING", ["-Iruntime/orchestrator/include", "-Isdk/game/include"],
         ["--no-recursive-allowlist",
          "--allowlist-function", "rsf_native_(cpu_(event|dispatch|read|release|release_session|marker|ping_message|counters_get|rendered_source_set|reservation_read|reservation_counters_get|present_result)|input_receipts_get)",
          "--raw-line", "use crate::cpu_abi::{\n"
          "    rsf_native_cpu_counters, rsf_native_cpu_frame, rsf_native_cpu_reservation,\n"
          "    rsf_native_cpu_reservation_counters, rsf_native_input_receipts,\n};",
          "--raw-line", "use rsf_game_sys::{rsf_game_cpu_event, rsf_game_cpu_event_fn};"])
generate("runtime/orchestrator/src/scene_abi.rs", "runtime/orchestrator/include/rescaleframe/native_window.h",
         "rsf_(native_present_record|observer_present_event|native_scene_identity)",
         "RSF_NATIVE_SCENE_PRESENT_.*",
         ["-include", "rescaleframe/native_scene.h", "-Iruntime/orchestrator/include",
          "-Iruntime/graphics/include", "-Isdk/game/include"],
         ["--with-derive-default"])
generate("runtime/graphics/src/d3d11_observer_layout_abi.rs", "runtime/graphics/include/rescaleframe/d3d11_observer.h",
         "rsf_observer_layout_element", "NOTHING", ["-Iruntime/graphics/include"],
         ["--no-recursive-allowlist", "--with-derive-default"])
generate("runtime/orchestrator/src/d3d11_observer_abi.rs", "runtime/graphics/include/rescaleframe/d3d11_observer.h",
         "rsf_(observer_(options|status|(layout|shader|texture|buffer|present|present_event)_fn)|dump_log_fn)",
         "RSF_OBSERVER_(ABI_VERSION|MAX_LAYOUT_ELEMENTS|STAGE_.*)", ["-Iruntime/graphics/include"],
         ["--no-recursive-allowlist", "--raw-line", "pub use crate::d3d11_observer_layout_abi::rsf_observer_layout_element;",
          "--raw-line", "use crate::scene_abi::rsf_observer_present_event;"])
generate("games/ac7/src/motion_capture_graphics_abi.rs", "runtime/graphics/include/rescaleframe/frame_tap.h",
         "rsf_(frame_tap_(geometry|target_draw|input|options|result|target_fn|compute_fn)|"
         "observer_(options|result)|texture_dump_(options|report)|dump_texture_result|constant_buffer_result)",
         "RSF_(FRAME_TAP_(ABI_VERSION|OK)|OBSERVER_(ABI_VERSION|OK)|TEXTURE_(DUMP_ABI_VERSION|OK)|CONSTANT_BUFFER_OK)",
         ["-Iruntime/graphics/include", "-Igames/ac7/include", "-Isdk/game/include",
          "-include", "rescaleframe/ac7_motion_capture.h", "-include", "rescaleframe/d3d11_observer.h",
          "-include", "rescaleframe/constant_buffer_read.h"],
         ["--with-derive-default", "--raw-line", "#![allow(dead_code)]",
          "--allowlist-function", "rsf_(ac7_motion_capture_.*|frame_tap_(install|uninstall|set_constant_watch|set_research(_phase)?_callbacks)|"
          "observer_(install|uninstall)|dump_texture(_bytes)?|read_constant_buffer)",
          "--blocklist-type", "rsf_(observer_layout_element|game_render_pass)",
          "--raw-line", "use crate::d3d11_observer_layout_abi::rsf_observer_layout_element;",
          "--raw-line", "use rsf_game_sys::rsf_game_render_pass;"])
generate("runtime/orchestrator/src/d3d12_observer_abi.rs", "runtime/graphics/include/rescaleframe/d3d12_observer.h",
         "rsf_d3d12_observer_setup", "NOTHING", [])

generate("runtime/orchestrator/src/frame_tap_abi.rs", "runtime/graphics/src/frame_tap_platform.h",
         "rsf_frame_tap_.*", "RSF_FRAME_TAP_.*", ["-Iruntime/graphics/include"],
         ["--no-recursive-allowlist", "--raw-line", "#![allow(dead_code)]",
          "--raw-line", "use crate::resource_roles_abi::{rsf_role_table, rsf_texture_facts};"])
generate("runtime/orchestrator/src/d3d11_output_abi.rs", "runtime/orchestrator/src/d3d11_output_platform.h",
         "rsf_(native_composition_frame|fullscreen_pass|d3d11_state)", "RSF_D3D11_(FORMAT_.*|USAGE_.*|BIND_.*)",
         ["-include", "rescaleframe/native_composition.h", "-Iruntime/orchestrator/include",
          "-Iruntime/graphics/include", "-Isdk/game/include"],
         ["--with-derive-default", "--no-copy", "rsf_d3d11_state",
          "--allowlist-function", "rsf_d3d11_output_.*"])
generate("sdk/game/src/d3d11_host_abi.rs", "runtime/orchestrator/include/rescaleframe/d3d11_host.h",
         "rsf_d3d11_(auxiliary_(surface|pass|scope)|host_capture|sampler_bias|host_setup|creation_scope|native_chain|present_dispatch|presentation_binding|capture_control)",
         "RSF_D3D11_(AUXILIARY_.*|HOST_CAP_.*|CAPTURE_.*)",
         ["-Iruntime/orchestrator/include", "-Iruntime/graphics/include",
          "-Iruntime/presentation/include", "-Isdk/game/include"],
         ["--no-recursive-allowlist", "--with-derive-default",
          "--raw-line", "type wchar_t = u16;",
          "--raw-line", "use crate::rsf_game_render_pass;",
          "--raw-line", "use crate::sr_abi::rsf_motion_decode_params;",
          "--with-derive-custom-struct", "rsf_d3d11_host_setup=Copy,Clone",
          "--raw-line", "mod upstream;", "--raw-line", "pub use upstream::same_upstream_identity;"])
generate("tests/game-sdk/src/d3d11_host_calls.rs", "runtime/orchestrator/include/rescaleframe/d3d11_host.h",
         "NOTHING", "NOTHING",
         ["-include", "rescaleframe/native_sr.h", "-include", "rescaleframe/native_fg.h",
          "-Iruntime/orchestrator/include", "-Iruntime/graphics/include",
          "-Iruntime/presentation/include", "-Iruntime/backends/dlss/include",
          "-Iruntime/contract/include", "-Isdk/game/include"],
         ["--no-recursive-allowlist",
          "--allowlist-function", "rsf_(d3d11_host_(create|set_active|dispatch_chain|dispatch_present|creation_begin|creation_end|wrap_native_swapchain|bind_presentation|request_capture|capture_control|capture_texture|restore_auxiliaries|reduced_frame)|native_fg_(scene|scene_swap|bind_source)|native_sr_capture_fg)",
          "--raw-line", "use rsf_game_sys::d3d11_chain_abi::rsf_d3d11_chain_dispatch;",
          "--raw-line", "use rsf_game_sys::d3d11_host_abi::*;",
          "--raw-line", "use rsf_game_sys::d3d11_reduced_abi::rsf_d3d11_reduced_frame;",
          "--raw-line", "use rsf_game_sys::sr_abi::rsf_motion_decode_params;",
          "--raw-line", "use rsf_game_sys::{rsf_game_host_services, rsf_game_render_pass};"])
generate("runtime/orchestrator/src/auxiliary_sr_abi.rs", "runtime/orchestrator/src/auxiliary_sr_platform.h",
         "rsf_(aux_platform|depth_scale_(setup|frame))",
         "RSF_(AUX_.*|DEPTH_SCALE_(ABI_VERSION|OK))",
         ["-Iruntime/orchestrator/include", "-Iruntime/graphics/include",
          "-Iruntime/presentation/include", "-Isdk/game/include"],
         ["--blocklist-type", "rsf_game_log_fn",
          "--blocklist-type", "rsf_d3d11_auxiliary_surface",
          "--blocklist-type", "rsf_d3d11_state",
          "--raw-line", "use crate::d3d11_output_abi::rsf_d3d11_state;",
          "--raw-line", "pub use rsf_game_sys::d3d11_reduced_abi::{\n"
          "    RSF_D3D11_AUXILIARY_COLOUR, RSF_D3D11_AUXILIARY_DEPTH_STENCIL,\n"
          "    RSF_D3D11_AUXILIARY_RESTORE_CONTENTS, rsf_d3d11_auxiliary_surface,\n};",
          "--raw-line", "use rsf_game_sys::rsf_game_log_fn;",
          "--with-derive-default"])
generate("runtime/orchestrator/src/graphics_pass_abi.rs", "runtime/graphics/include/rescaleframe/fullscreen_pass.h",
         "rsf_(fullscreen_(setup|draw)|present_blit_setup|motion_decode_setup|depth_replay_detail)",
         "RSF_(FULLSCREEN_PASS|PRESENT_BLIT|MOTION_DECODE)_ABI_VERSION",
         ["-Iruntime/graphics/include", "-include", "rescaleframe/motion_decode.h",
          "-include", "rescaleframe/present_blit.h", "-include", "rescaleframe/depth_replay.h"],
         ["--with-derive-default"])
generate("tools/exposure_probe/src/abi.rs", "runtime/backends/dlss/include/rescaleframe/dlss.h",
         "rsf_(dlss_(frame|setup)|texture_dump_options)",
         "RSF_(DLSS_(ABI_VERSION|ENGINE_UNREAL|QUALITY_PERFORMANCE)|TEXTURE_DUMP_ABI_VERSION)",
         ["-Iruntime/backends/dlss/include", "-Iruntime/graphics/include",
          "-include", "rescaleframe/texture_dump.h"],
         ["--with-derive-default", "--allowlist-function",
          "rsf_(dlss_(load|set_device|evaluate|shutdown)|dump_texture_bytes)"])
generate("runtime/orchestrator/src/native_sr_abi.rs", "runtime/orchestrator/include/rescaleframe/native_sr.h",
         "rsf_dlss_pipeline_frame", "NOTHING",
         ["-include", "rescaleframe/native_translucency.h", "-Iruntime/orchestrator/include",
          "-Iruntime/backends/dlss/include", "-Iruntime/graphics/include", "-Iruntime/contract/include",
          "-Isdk/game/include"],
         ["--blocklist-type", "rsf_pipeline_camera_frame",
          "--raw-line", "use crate::render_abi::rsf_pipeline_camera_frame;",
          "--raw-line", "pub use rsf_game_sys::sr_abi::{RSF_DLSS_PIPELINE_ABI_VERSION, rsf_motion_decode_params};"])
overlay_wire = "rsf_overlay_(host_option|intent|quality|saved_row|stats)"
generate("ui/overlay/src/wire.rs", "ui/overlay/include/rescaleframe/overlay.h",
         overlay_wire, "RSF_OVERLAY_(ABI_VERSION|QUALITY_ULTRA_QUALITY|SR_QUALITY_.*|SR_CHANGE_.*|FRAME_LIMIT_VRR|SAVED_.*)",
         [], ["--with-derive-default", "--with-derive-partialeq", "--no-default", "rsf_overlay_intent",
              "--raw-line", "#![allow(missing_docs, dead_code)]"], derive_debug=True)
generate("runtime/orchestrator/src/overlay_saved_abi.rs", "runtime/orchestrator/include/rescaleframe/overlay_saved_options.h",
         "rsf_overlay_saved_(option|setup|apply)", "RSF_OVERLAY_SAVED_(OPTIONS_ABI_VERSION|TEXT_LIMIT|TOGGLE|NUMBER|TEXT)",
         ["-Iruntime/orchestrator/include", "-Iui/overlay/include"],
         ["--with-derive-default", "--raw-line", "#![allow(dead_code)]"])
generate("ui/overlay/src/fg_reason_codes.rs", "runtime/orchestrator/include/rescaleframe/native_fg.h",
         "NOTHING", "RSF_NATIVE_FG_REASON_.*",
         ["-Iruntime/orchestrator/include", "-Iruntime/contract/include", "-Iruntime/presentation/include",
          "-Iruntime/graphics/include", "-Iruntime/backends/dlss/include", "-Isdk/game/include"])
generate("tests/overlay-service/src/abi.rs", "runtime/orchestrator/include/rescaleframe/overlay_api.h",
         "rsf_(overlay_api|overlay_service_setup|overlay_diagnostics|render_status|native_fg_status)",
         "RSF_(OVERLAY_(API_VERSION|CAPTURE_.*|NOTIFY_.*|SERVICE_OPTION_.*|SR_INPUT_.*)|RENDER_FRONTEND_ABI_VERSION|"
         "FG_(BACKEND_(AUTO|FSR3|DLSS)|FIXED)|REFLEX_(OFF|ON|BOOST))",
         ["-include", "rescaleframe/native_fg.h",
          "-Iruntime/orchestrator/include", "-Iruntime/contract/include", "-Iruntime/presentation/include",
          "-Iruntime/graphics/include", "-Iruntime/backends/dlss/include", "-Isdk/game/include"],
         ["--with-derive-default", "--raw-line", "#![allow(dead_code)]"])

generate("runtime/orchestrator/src/native_translucency_abi.rs",
         "runtime/orchestrator/include/rescaleframe/native_translucency.h",
         "rsf_native_translucency_masks", "NOTHING",
         ["-Iruntime/orchestrator/include", "-Iruntime/backends/dlss/include",
          "-Iruntime/graphics/include", "-Iruntime/contract/include", "-Isdk/game/include"],
         ["--with-derive-default"])
# Match the overlay's existing std paths and import types that other modules already own.
generate("runtime/orchestrator/src/overlay_abi.rs", "runtime/orchestrator/src/overlay_platform.h",
         "wchar_t|rsf_overlay_(api|service_setup|service_option|capture|diagnostics|"
         "input_counters|platform|texture_update)|rsf_native_(latency_status|input_routes)|rsf_render_status",
         "RSF_OVERLAY_(API_VERSION|CAPTURE_.*|NOTIFY_.*|SERVICE_OPTION_(TOGGLE|NUMBER|LIMIT)|"
         "HOST_OPTION_(TOGGLE|NUMBER)|FG_RUNTIME_SWITCH)|RSF_(REFLEX_ON|SR_NONE|SR_DLSS|SR_FSR3|SR_FSR4|SR_SPATIAL)",
         ["-include", "rescaleframe/overlay_api.h", "-include", "rescaleframe/overlay.h",
          "-include", "rescaleframe/overlay_input.h", "-include", "rescaleframe/native_fg.h",
          "-include", "rescaleframe/native_cpu.h", "-include", "rescaleframe/sr_session.h",
          "-Isdk/game/include", "-Iruntime/orchestrator/include", "-Iruntime/contract/include",
          "-Iruntime/presentation/include", "-Iruntime/graphics/include",
          "-Iruntime/backends/dlss/include", "-Iui/overlay/include"],
         ["--no-recursive-allowlist", "--with-derive-default",
          "--raw-line", "#[path = \"../../../ui/overlay/src/wire.rs\"]",
          "--raw-line", "pub(crate) mod overlay_wire;",
          "--raw-line", "pub use overlay_wire::{\n"
                        "    RSF_OVERLAY_FRAME_LIMIT_VRR, RSF_OVERLAY_SR_CHANGE_APPLIED, RSF_OVERLAY_SR_CHANGE_PENDING,\n"
                        "    RSF_OVERLAY_SR_CHANGE_REFUSED, RSF_OVERLAY_SR_QUALITY_EXTENDED_MASK,\n"
                        "    RSF_OVERLAY_SR_QUALITY_STANDARD_MASK, rsf_overlay_host_option, rsf_overlay_intent,\n"
                        "    rsf_overlay_stats,\n};",
          "--raw-line", "impl Default for rsf_overlay_intent {\n"
                        "    fn default() -> Self {\n"
                        "        unsafe { std::mem::zeroed() }\n"
                        "    }\n"
                        "}",
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
         "rsf_overlay(12_(log|platform|target|draw|draw_call|frame)|11on12_(device|target|platform))", "RSF_OVERLAY12_TEXTURE_LIMIT",
         ["-Iruntime/orchestrator/include", "-Iui/overlay/include", "-Isdk/game/include"],
         ["--no-recursive-allowlist", "--raw-line",
          "use crate::overlay_abi::{rsf_overlay_intent, rsf_overlay_stats, rsf_overlay_texture_update};",
          "--raw-line", "use crate::overlay_host_abi::rsf_overlay_input;"])
generate("tests/overlay/src/abi.rs", "loader/proxy/src/overlay_host.h",
         "rsf_(overlay_(input(_options|_counters)?|intent|stats)|observer_options)",
         "RSF_(OVERLAY_(INPUT_[A-Z_]+|MOUSE_LEFT)|OBSERVER_(ABI_VERSION|OK|ERROR_NO_DEVICE))",
         ["-include", "rescaleframe/overlay_input.h", "-include", "rescaleframe/d3d11_observer.h",
          "-Iui/overlay/include", "-Iruntime/graphics/include"],
         ["--allowlist-function", "rsf_(overlay_(input_(install|uninstall|visible|poll_toggle|set_visible|collect|counters_get)|host_(start|present|visible|stop))|observer_(install|uninstall|acquire_device))",
          "--with-derive-default", "--raw-line", "#![allow(dead_code)]"])
generate("runtime/orchestrator/src/overlay_host_abi.rs", "runtime/orchestrator/src/overlay_panel.h",
         "rsf_overlay(_result|_input(_options|_log_fn)?|_draw_data|_vertex|_draw_call|_host_panel)?",
         "RSF_OVERLAY_(ABI_VERSION|INPUT_ABI_VERSION|HOST_PANEL_.*)",
         ["-include", "rescaleframe/overlay_input.h", "-Iui/overlay/include", "-Iruntime/graphics/include"],
         ["--no-recursive-allowlist", "--with-derive-default", "--raw-line",
          "use crate::overlay_abi::{rsf_overlay_intent, rsf_overlay_stats, rsf_overlay_texture_update};"])
generate("runtime/orchestrator/src/shader_patch_abi.rs", "runtime/shader_patch/src/json.h",
         "rsf_patch_json_visit", "RSF_PATCH_JSON_.*",
         ["-Iruntime/shader_patch/include"],
         ["--no-recursive-allowlist", "--raw-line", "pub(crate) use rsf_game_sys::shader_patch_abi::*;"])
generate("runtime/orchestrator/src/ui_identify_abi.rs", "runtime/graphics/include/rescaleframe/ui_identify.h",
         "rsf_ui_(set|result|registry_counters)", "RSF_UI_(IDENTIFY_ABI_VERSION|MAX_.*)", [])
generate("runtime/orchestrator/src/ui_layer_abi.rs", "runtime/graphics/include/rescaleframe/ui_layer.h",
         "rsf_ui_layer_(setup|status|result|log_fn)", "RSF_UI_LAYER_(ABI_VERSION|RING)", [])
generate("runtime/orchestrator/src/vulkan_sr_abi.rs", "runtime/orchestrator/include/rescaleframe/native_sr_split_vulkan.h",
         "rsf_(srvk_(setup|frame|image|fallback|fsr4_provider_info)|fsr4vk_handoff|vk_(function|get_proc)|input_dump_vk_setup)",
         "RSF_(SRVK_(ABI_VERSION|SPLIT_ABI_VERSION|SPLIT_TRACKED_ABI_VERSION|FSR4_INFO_ABI_VERSION)|SR_(DLSS|FSR2|FSR3|FSR4|XESS)|FSR4_BRIDGE_(INVALID_ARGUMENT|ABI_MISMATCH))",
         ["-include", "runtime/vulkan/fsr4/bridge_api.h", "-include", "rescaleframe/input_dump_vulkan.h",
          "-Iruntime/orchestrator/include",
          "-Iruntime/contract/include", "-Iruntime/graphics/include", "-Isdk/game/include"],
         ["--no-recursive-allowlist", "--raw-line", "use crate::render_abi::rsf_sharpening_options;",
          "--raw-line", "use rsf_game_sys::rsf_frame_record;"])
generate("tests/presentation/src/gl_fixture_abi.rs", "runtime/presentation/include/rescaleframe/gl_shared_surface.h",
         "rsf_gl_(context|texture|setup|texture_info)", "RSF_GL_.*",
         ["-Iruntime/presentation/include", "-Iruntime/contract/include", "-Isdk/game/include"],
         ["--allowlist-function", "rsf_gl_(open|graphics|texture_(create|get|destroy)|begin|submit|drain|close)",
          "--with-derive-default"])
generate("tests/presentation/src/gl_pipeline_fixture_abi.rs", "runtime/orchestrator/include/rescaleframe/gl_pipeline.h",
         "rsf_(gl_pipeline(_(setup|options|surfaces|status))?|native_latency_status|overlay_(api|diagnostics)|"
         "render_settings|streamline_host)", "RSF_(OVERLAY_API_VERSION|RENDER_FRONTEND_ABI_VERSION|FG_(OFF|FIXED)|REFLEX_(OFF|ON)|SR_(DLSS|FSR3))",
         ["-include", "rescaleframe/input_dump.h", "-include", "rescaleframe/native_cpu.h",
          "-include", "rescaleframe/overlay_api.h", "-include", "rescaleframe/d3d11_present_bridge.h",
          "-include", "rescaleframe/sr_session.h",
          "-Iruntime/orchestrator/include", "-Iruntime/backends/dlss/include", "-Iruntime/graphics/include",
          "-Iruntime/contract/include", "-Iruntime/presentation/include", "-Isdk/game/include"],
         ["--blocklist-type", "rsf_(game_render_pass|game_render_pass_role|camera_frame)",
          "--with-derive-default", "--raw-line", "#![allow(dead_code)]",
          "--raw-line", "use rsf_game_sys::rsf_game_render_pass;",
          "--allowlist-function", "rsf_(gl_window_(create|destroy)|gl_pipeline_(open|configure(_settings)?|"
          "settings_(open|revision)|get_surfaces|cpu|input|pcl_message|abort|evaluate|hudless|present|status_get|close)|"
          "native_latency_status_get|d3d11_present_host|streamline_host_token|input_dump_(configure|request)|"
          "get_overlay_(api|diagnostics))"])
generate("runtime/orchestrator/src/gl_pipeline_abi.rs", "runtime/orchestrator/include/rescaleframe/gl_pipeline.h",
         "rsf_(gl_pipeline_(setup|options|surfaces|status)|gl_(setup|texture_info)|sr12_setup|dlss_setup)",
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
generate("runtime/orchestrator/src/d3d11_fg_api_abi.rs", "runtime/orchestrator/include/rescaleframe/d3d11_fg_api.h",
         "rsf_d3d11_fg_(api|inputs)", "RSF_D3D11_FG_(API|INPUTS)_VERSION",
         ["-Iruntime/orchestrator/include", "-Iruntime/contract/include", "-Iruntime/backends/dlss/include",
          "-Iruntime/graphics/include", "-Iruntime/presentation/include", "-Isdk/game/include"],
         ["--no-recursive-allowlist", "--raw-line", "type rsf_streamline_host = ::core::ffi::c_void;",
          "--raw-line", "use crate::render_abi::{rsf_backend_log_fn, rsf_native_fg_options, rsf_native_fg_status};",
          "--raw-line", "use crate::scene_abi::rsf_observer_present_event;",
          "--raw-line", "use rsf_game_sys::{rsf_game_cpu_event, rsf_game_render_pass};"])
generate("runtime/orchestrator/src/native_fg_abi.rs", "runtime/orchestrator/src/native_fg_platform.h",
         "rsf_(nfg_texture_info|named_frame_dump(_report|_log_fn)?)",
         "RSF_(NFG_(FORMAT_.*|STATE_.*|PHASE_.*|DUMP_VIEW_.*|SR_.*|FG_BACKEND_.*|STAT_.*|PRESENT_.*|RESULT_.*)|NAMED_FRAME_DUMP_ABI_VERSION)",
         ["-Iruntime/orchestrator/include", "-Iruntime/contract/include", "-Iruntime/backends/dlss/include",
          "-Iruntime/graphics/include", "-Iruntime/presentation/include", "-Isdk/game/include"],
         ["--no-recursive-allowlist"])
generate("runtime/orchestrator/src/native_fg_cpu_abi.rs", "runtime/orchestrator/src/native_fg_cpu_platform.h",
         "rsf_(nfg_cpu_platform|d3d11_present_status|streamline_latency_status|native_latency_window)",
         "RSF_(NFG_CPU_.*|PRESENT_STATUS_VERSION|SL_PROFILE_PCL|PACING_ANTILAG2)",
         ["-Iruntime/orchestrator/include", "-Iruntime/contract/include", "-Iruntime/backends/dlss/include",
          "-Iruntime/graphics/include", "-Iruntime/presentation/include", "-Isdk/game/include"],
         ["--no-recursive-allowlist",
          "--raw-line", "use crate::render_abi::{rsf_fg_status, rsf_pacing_owner, rsf_reflex_mode};",
          "--raw-line", "use rsf_game_sys::rsf_frame_record;"])

generate("tests/game-sdk/src/fg_cpu_calls.rs", "runtime/orchestrator/src/native_fg_cpu_platform.h",
         "rsf_native_(latency_frame|input_routes)", "RSF_FG_BACKEND_XESS",
         ["-Iruntime/orchestrator/include", "-Iruntime/contract/include", "-Iruntime/backends/dlss/include",
          "-Iruntime/graphics/include", "-Iruntime/presentation/include", "-Isdk/game/include"],
         ["--no-recursive-allowlist", "--with-derive-default",
          "--allowlist-function", "rsf_(core_(nfg_(duplicate_render_thread|sample_render_thread|describe_address)|native_(fg_(cpu|frame|reflex_ordering_set)|input_routes_get|latency_(render|present|frame_get|window_bind|retire|end_session)))|native_(fg_rendered_source_set|latency_has_claim))",
          "--allowlist-var", "rsf_native_fg_cpu_platform",
          "--raw-line", "use crate::native_fg_cpu_abi::{rsf_native_latency_window, rsf_nfg_cpu_platform};",
          "--raw-line", "use rsf_game_sys::{rsf_game_cpu_event, rsf_game_render_pass};"])

generate("tests/generation/src/abi.rs", "tests/generation/fixture_abi.h",
         "rsf_(fg_choice|fg_retirement|pipeline_camera_frame|frame_resources|dlss_frame|dlss_quality)",
         "RSF_(FG_BACKEND_(ALL|AUTO|DLSS|FSR3|XESS)|SR_(NONE|DLSS|FSR3|FSR4|XESS)|"
         "FRAME_ASSEMBLY_ABI_VERSION|DLSS_ABI_VERSION|TEST_DLSS_.*)",
         ["-Iruntime/orchestrator/include", "-Iruntime/contract/include", "-Iruntime/backends/dlss/include",
          "-Iruntime/graphics/include", "-Iruntime/presentation/include", "-Isdk/game/include"],
         ["--no-recursive-allowlist"])

generate("tests/capture/src/abi.rs", "tests/capture/capture_fixture_abi.h",
         "rsf_(fixture_vk_capture_api|input_dump(12|_vk|_vk_setup)|dump_(log_fn|pixels|view|texture_result)|"
         "texture_dump_(options|motion_options|report)|srvk_(setup|image)|vk_(function|get_proc)|"
         "fg_frame|dlss_(frame|quality)|sr_frame|backend_resource|named_frame_dump(_report|_log_fn)?|lifetime)",
         "RSF_(FIXTURE_.*|DUMP_VIEW_.*|INPUT_DUMP_VK_DEVICE_LOST|TEXTURE_.*|BACKEND_.*|SRVK_ABI_VERSION)",
         ["-Isdk/game/include", "-Iruntime/orchestrator/include", "-Iruntime/contract/include",
          "-Iruntime/graphics/include", "-Iruntime/backends/dlss/include"],
         ["--no-recursive-allowlist", "--with-derive-default", "--no-prepend-enum-name",
          "--allowlist-function", "rsf_(input_dump.*|dump_texture.*|dump_pixels_png|native_window_(scope|reset))",
          "--raw-line", "use rsf_game_sys::{rsf_frame_record, rsf_game_render_pass};"])

generate("tests/generation/src/session_abi.rs", "tests/generation/session_abi.h",
         "rsf_(fg_(host|session_status|swapchain_desc|frame|options|status|mode)|generation_(setup|provider)|"
         "sequence_state|backend_(resource|result|log_fn)|gfx_api|lifetime|ui_mode|latency_(mode|marker)|"
         "pacing_owner|reflex_mode|streamline_(host_setup|graphics))",
         "RSF_(FG_ABI_VERSION|FG_(OFF|FIXED|DYNAMIC)|FG_BACKEND_(DLSS|FSR3|XESS)|BACKEND_ABI_VERSION|"
         "PACING_(NONE|REFLEX)|REFLEX_(ON|OFF)|UI_MODE_NONE|STREAMLINE_HOST_ABI_VERSION|SL_PROFILE_DLSS_FG)",
         ["-Iruntime/orchestrator/include", "-Iruntime/contract/include", "-Iruntime/backends/dlss/include",
          "-Iruntime/graphics/include", "-Iruntime/presentation/include", "-Isdk/game/include"],
         ["--no-recursive-allowlist",
          "--raw-line", "use crate::abi::rsf_fg_retirement;",
          "--raw-line", "use rsf_game_sys::rsf_frame_record;"])

generate("tests/vulkan-fg/src/wsi_abi.rs", "tests/vulkan-fg/wsi_abi.h",
         "rsf_wsi_fixture_(counts|calls)", "NOTHING", [])

for fixture in ("dispatch", "ffx"):
    symbols = (r"rsf_(fgvk_.*|core_fgvk_presentation_.*|fixture_(dispatch_state|block_sleep|wait_sleep|release_sleep))"
               if fixture == "dispatch" else
               r"rsf_(ffxgvk_.*|fixture_(ffx|wsi)_.*|core_vkfg_(ensure_image|release_image))")
    generate(f"tests/vulkan-fg/src/{fixture}_abi.rs", f"tests/vulkan-fg/{fixture}_abi.h",
             "rsf_fixture_result", "RSF_(FGVK_.*|SRVK_ABI_VERSION|FSR4FG_NOT_SUPPORTED)",
             ["-Iruntime/orchestrator/include", "-Iruntime/contract/include", "-Isdk/game/include",
              *(["-x", "c++", "-std=c++17", "-Wno-pragma-once-outside-header"] if fixture == "ffx" else [])],
             ["--allowlist-function", symbols, "--blocklist-function", "rsf_fixture_.*_run",
              "--blocklist-type", "rsf_frame_record",
              "--raw-line", "#![allow(dead_code)]",
              "--raw-line", "use rsf_game_sys::rsf_frame_record;",
              "--no-prepend-enum-name", "--with-derive-default"])

generate("tests/generation/src/dll_abi.rs", "tests/generation/dll_abi.h",
         "rsf_(d3d11_(fg_(api|inputs)|present_api)|get_d3d11_(fg|present)_api_fn|fg_fixture_imports|"
         "native_(fg_(options|status)|cpu_frame)|observer_present_event)",
         "RSF_D3D11_(FG_(API|INPUTS)_VERSION|PRESENT_API_VERSION)",
         ["-Iruntime/orchestrator/include", "-Iruntime/contract/include", "-Iruntime/backends/dlss/include",
          "-Iruntime/graphics/include", "-Iruntime/presentation/include", "-Isdk/game/include"],
         ["--no-recursive-allowlist",
          "--raw-line", "use crate::session_abi::{\n"
          "    rsf_backend_log_fn, rsf_fg_status, rsf_generation_provider, rsf_latency_marker,\n"
          "    rsf_streamline_graphics,\n};",
          "--raw-line", "use rsf_game_sys::{rsf_game_cpu_event, rsf_game_render_pass};",
          "--raw-line", "type rsf_result = i32;",
          "--raw-line", "type rsf_streamline_host = ::core::ffi::c_void;",
          "--raw-line", "type rsf_d3d11_present_setup = ::core::ffi::c_void;"])

generate("integrations/godot/native/host_abi.rs", "integrations/godot/native/host_abi.h",
         "rsf_(sr12(_setup)?|dlss_setup|backend_(result|log_fn)|dlss_(log_fn|engine)|"
         "fg12_setup|native_(fg_(options|status)|cpu_counters|latency_status)|"
         "fg_status|pacing_owner|reflex_mode|fg_mode|streamline_host|vk_wsi_call)",
         "RSF_DLSS_ABI_VERSION",
         ["-Iruntime/orchestrator/include", "-Iruntime/contract/include",
          "-Iruntime/backends/dlss/include", "-Iruntime/graphics/include",
          "-Iruntime/presentation/include", "-Isdk/game/include"],
         ["--no-recursive-allowlist", "--with-derive-default",
          "--allowlist-function", "rsf_(sr12_(create|plan|evaluate|set_sharpening|destroy)|"
          "fg12_(install_explicit|options|status|hudless|render_begin|end_session)|"
          "d3d11_present_(backend|generation|switch_result|has_owner|request|host|create_for_hwnd)|"
          "gpu_default_sr_backend|native_cpu_counters_get|native_latency_status_get|"
          "input_dump_(cancel|request)|slvk_without_dlssg|latencyvk_(queue_present|wsi_call))",
          "--raw-line", "use crate::vulkan::creation::abi::rsf_vk_function;",
          "--raw-line", "use rsf_game_sys::{rsf_game_render_pass, sharpening_abi::rsf_sharpening_options};"])

generate("tests/generation/src/latency_abi.rs",
         "tests/generation/latency_abi.h",
         "rsf_streamline_latency_status",
         "RSF_SL_PROFILE_(REFLEX|PCL)|LATENCY_.*",
         ["-Iruntime/backends/dlss/include", "-Iruntime/contract/include", "-Isdk/game/include"],
         ["--no-recursive-allowlist", "--with-derive-default", "--no-prepend-enum-name",
          "--raw-line", "use crate::session_abi::{rsf_pacing_owner, rsf_reflex_mode};"])

generate("runtime/orchestrator/src/streamline_generation_records_abi.rs",
         "runtime/backends/dlss/src/streamline_generation_records.h",
         "rsf_slhost_(tag|reflex_sample)", "NOTHING", [])

generate("runtime/orchestrator/src/dlss_pipeline_layer_abi.rs",
         "runtime/orchestrator/include/rescaleframe/dlss_pipeline.h",
         "^rsf_dlss_pipeline_layer$", "^RSF_DLSS_PRESET_MASK$",
         ["-Iruntime/orchestrator/include", "-Iruntime/backends/dlss/include",
          "-Iruntime/contract/include", "-Iruntime/graphics/include",
          "-Iruntime/presentation/include", "-Isdk/game/include"],
         ["--no-recursive-allowlist", "--raw-line",
          "use crate::render_abi::rsf_pipeline_camera_frame;"])

generate("sdk/game/src/render_host_abi.rs", "runtime/orchestrator/include/rescaleframe/render_host.h",
         "rsf_render_host_setup", "RSF_RENDER_HOST_(ABI_VERSION|ENGINE_PRESENTATION|OWNS_PRESENTATION)",
         ["-Isdk/game/include", "-Iruntime/orchestrator/include"],
         ["--no-recursive-allowlist", "--with-derive-default", "--raw-line", "type wchar_t = u16;"])

generate("tests/game-sdk/src/render_host_fixture_abi.rs", "tests/game-sdk/render_host_fixture_abi.h",
         "rsf_(render_(settings|status|links?)|native_present_record|observer_present_event)",
         "RSF_(RENDER_(FRONTEND_ABI_VERSION|LINK_ABI_VERSION)|SR_(AUTO|DLSS|FSR3|FSR4|SPATIAL|XESS))",
         ["-Isdk/game/include", "-Iruntime/orchestrator/include", "-Iruntime/contract/include",
          "-Iruntime/graphics/include", "-Iruntime/presentation/include", "-Iruntime/backends/dlss/include"],
         ["--no-recursive-allowlist", "--with-derive-default",
          "--allowlist-function", "rsf_(callback_host_(start|stop)|native_scene_(pass|reset|texture_binding)|"
          "native_window_(scope|match|present|last_present|reset)|render_(host_create|settings_(get|apply)|status_get|links_.*))",
          "--raw-line", "use rsf_game_sys::{\n"
          "    callback_host_abi::rsf_callback_host_setup, render_host_abi::rsf_render_host_setup,\n"
          "    rsf_game_host_services, rsf_game_render_pass,\n};",
          "--raw-line", "type rsf_backend_result = i32;"])

generate("runtime/orchestrator/src/ffx_version_abi.rs", "runtime/backends/fsr/src/ffx_version.h",
         "rsf_ffx_version", "RSF_FFX_MAX_VERSIONS", [], ["--with-derive-default"])

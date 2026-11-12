#!/usr/bin/env python3
"""Read local Anomaly assets without redistributing them. No runtime/deploy support.

Inventory is lexical, including conditional branches, not HLSL preprocessing or
compiler reflection. Paths follow the engine's case-insensitive include lookup.
"""
import argparse
from collections import Counter
import hashlib
import json
from pathlib import Path, PurePosixPath
import re


class Refusal(ValueError):
    """An input or destination did not match the bounded operation."""


def digest(data):
    return hashlib.sha256(data).hexdigest()


def uncomment(text, lua=False):
    pattern = r'"[^"\n]*"|\'[^\'\n]*\'|//[^\n]*|/\*[\s\S]*?\*/'
    if lua:
        pattern = r'"[^"\n]*"|\'[^\'\n]*\'|--\[\[[\s\S]*?\]\]|--[^\n]*'
    return re.sub(pattern, lambda m: re.sub(r"[^\n]", " ", m[0])
                  if m[0].startswith(("//", "/*", "--")) else m[0], text)


def family(path):
    name = PurePosixPath(path).name
    for prefix, label in (
        ("deffer_impl", "terrain"), ("deffer_base", "deferred_static"),
        ("deffer_model", "deferred_model"), ("deffer_tree_s", "tree_still"),
        ("deffer_tree", "tree_wave"), ("deffer_detail_w", "detail_wave"),
        ("deffer_detail_s", "detail_still"), ("shadow_", "shadow_only"),
        ("model_", "forward_model"), ("models_", "forward_model_material"),
        ("deffer_particle", "deferred_particle"), ("particle", "forward_particle"),
        ("water", "water"), ("sky2", "sky"), ("clouds", "clouds"),
        ("lod.", "lod_billboard"), ("pda_overlay", "pda_material"),
        ("hud", "hud"), ("combine_", "combine"),
    ):
        if name.startswith(prefix):
            return label
    if path.lower().startswith("dx11/tess"):
        return "tessellation"
    return "other"


def read_tree(root):
    root = Path(root).resolve(strict=True)
    blobs = {}
    folded = set()
    for path in sorted(root.rglob("*")):
        if path.is_symlink():
            raise Refusal(f"symlink not allowed: {path}")
        if not path.is_file():
            continue
        relative = path.relative_to(root).as_posix()
        if relative.casefold() in folded:
            raise Refusal(f"case-ambiguous path: {relative}")
        folded.add(relative.casefold())
        blobs[relative] = path.read_bytes()
    if not blobs:
        raise Refusal("empty asset tree")
    return blobs


def tree_digest(blobs):
    record = [[name, digest(data)] for name, data in sorted(blobs.items())]
    return digest(json.dumps(record, separators=(",", ":"), ensure_ascii=True).encode())


def inventory(root):
    blobs = read_tree(root)
    lookup = {name.casefold(): name for name in blobs}
    cleaned = {name: uncomment(data.decode("utf-8-sig", errors="replace"), lua=name.endswith(".s"))
               for name, data in blobs.items()}
    edges = {}
    for name, text in cleaned.items():
        edges[name] = []
        for match in re.finditer(r'^[ \t]*#[ \t]*include\s*"([^"\n]+)"', text, re.M):
            target = match[1].replace("\\", "/")
            # r4.cpp includer opens r3/<name>, then external $game_shaders$/<name>.
            # This inventory cannot resolve outside the supplied r3 corpus.
            resolved = lookup.get(target.casefold())
            edges[name].append({"name": target, "resolved": resolved,
                                "line": text.count("\n", 0, match.start()) + 1})

    files = []
    all_cycles = set()
    for name, data in blobs.items():
        closure = []
        seen = set()

        def visit(current, stack):
            if current in stack:
                # Include guards may make a lexical cycle harmless. Do not guess.
                all_cycles.add(tuple(stack[stack.index(current):] + [current]))
                return
            if current in seen:
                return
            seen.add(current)
            closure.append(current)
            for edge in edges[current]:
                if edge["resolved"]:
                    visit(edge["resolved"], stack + [current])
        visit(name, [])
        structs = {}
        for source in closure:
            for match in re.finditer(r'\bstruct\s+(\w+)\s*\{([^{}]*)\}\s*;', cleaned[source]):
                fields = [{"type": m[1], "name": m[2], "semantic": m[3]}
                          for m in re.finditer(r'\b(\w+)\s+(\w+)\s*(?:\[[^]]+\])?\s*:\s*(\w+)\s*;', match[2])]
                structs.setdefault(match[1], []).append({"struct": match[1], "source": source, "fields": fields})
        aliases = {}
        for source in closure:
            for match in re.finditer(r'^[ \t]*#[ \t]*define\s+(\w+)\s+(\w+)\s*$', cleaned[source], re.M):
                aliases.setdefault(match[1], set()).add(match[2])
        entrypoints = []
        for source in closure:
            for match in re.finditer(r'\b(\w+)\s+main\s*\(([^()]*)\)\s*(?::\s*\w+\s*)?\{', cleaned[source]):
                inputs = []
                def candidates(token, visited):
                    if token in visited:
                        return
                    visited.add(token)
                    inputs.extend(structs.get(token, []))
                    for alias in sorted(aliases.get(token, [])):
                        candidates(alias, visited)
                for token in re.findall(r'\b\w+\b', match[2]):
                    candidates(token, set())
                entrypoints.append({"source": source, "return_type": match[1],
                                    "parameters": match[2].strip(), "inputs": inputs})
        text = cleaned[name]
        files.append({"path": name, "sha256": digest(data), "size": len(data),
                      "stage": PurePosixPath(name).suffix.lstrip("."), "family": family(name),
                      "includes": edges[name], "dependency_closure": closure,
                      "entrypoints": entrypoints,
                      "conditional_directives": [{"line": text.count("\n", 0, m.start()) + 1,
                          "directive": m[0].strip()} for m in re.finditer(r'^[ \t]*#[ \t]*(?:if|ifdef|ifndef|elif|else|endif)\b[^\n]*', text, re.M)],
                      "defines": re.findall(r'^[ \t]*#[ \t]*define\s+(\w+)', text, re.M),
                      "tonemap_sites": [{"line": i, "symbols": sorted(set(re.findall(r'\b(?:tonemap|s_tonemap|Uncharted2ToneMapping)\b', line)))}
                                        for i, line in enumerate(text.splitlines(), 1)
                                        if re.search(r'\b(?:tonemap|s_tonemap|Uncharted2ToneMapping)\b', line)],
                      "textures": re.findall(r'\bTexture\w*(?:\s*<[^;]+?>)?\s+(\w+)', text),
                      "samplers": re.findall(r'\b(?:SamplerState|SamplerComparisonState|sampler)\s+(\w+)', text),
                      "material_samplers": re.findall(r'\bdx10sampler\s*\(\s*"([^"]+)"\s*\)', text),
                      "material_bindings": [{"sampler": m[1], "texture": m[2]}
                                            for m in re.finditer(r'\bdx10texture\s*\(\s*"([^"]+)"\s*,\s*"([^"]+)"\s*\)', text)]})
    return {"schema": 1, "runtime_ready": False,
            "semantic_status": "lexical_candidates_not_compiler_reflection",
            "file_count": len(files), "corpus_sha256": tree_digest(blobs),
            "family_counts": dict(sorted(Counter(x["family"] for x in files).items())),
            "include_cycles": [list(cycle) for cycle in sorted(all_cycles)],
            "missing_includes": [{"source": name, **edge} for name in edges for edge in edges[name] if edge["resolved"] is None],
            "files": files}


def inject_include(original, rule, include_name):
    """Only an exact original plus exact line can receive a declaration include."""
    if not re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*\.h", include_name):
        raise Refusal("include must be a plain HLSL header basename")
    if digest(original) != rule["source_sha256"]:
        raise Refusal("original source hash mismatch")
    lines = original.splitlines(keepends=True)
    line = rule["anchor_line"]
    if type(line) is not int or not 1 <= line <= len(lines):
        raise Refusal("anchor line outside source")
    anchor = lines[line - 1]
    if digest(anchor) != rule["anchor_sha256"]:
        raise Refusal("anchor hash mismatch")
    if not anchor.endswith(b"\n"):
        raise Refusal("anchor must end with a newline")
    newline = b"\r\n" if anchor.endswith(b"\r\n") else b"\n"
    insertion = b'#include "' + include_name.encode("ascii") + b'"' + newline
    return b"".join(lines[:line]) + insertion + b"".join(lines[line:])


def patch_plan(corpus):
    """Source-reviewed requirements, never an automatic semantic shader rewrite."""
    profile = json.loads(Path(__file__).with_name("anomaly_shader_vanilla_profile.json").read_text(encoding="utf-8"))
    if corpus["corpus_sha256"] != profile["corpus_sha256"] or corpus["file_count"] != profile["file_count"]:
        raise Refusal("source-reviewed planning requires the pinned vanilla corpus")
    requirements = {
        "deferred_static": ["previous_object_to_clip"],
        "terrain": ["previous_object_to_clip"],
        "deferred_model": ["previous_object_to_clip", "skin_mode_specific_previous_deformation", "separate_hud_projection_history"],
        "forward_model": ["previous_object_to_clip", "skin_mode_specific_previous_deformation", "transparent_coverage_policy", "separate_hud_projection_history"],
        "tree_wave": ["previous_world_to_clip", "previous_wave_wind_and_instance_transform"],
        "tree_still": ["previous_world_to_clip", "previous_instance_transform"],
        "detail_wave": ["previous_object_to_clip", "previous_per_instance_rows", "previous_wave_dir2D"],
        "detail_still": ["previous_object_to_clip", "previous_per_instance_rows"],
        "lod_billboard": ["previous_world_to_clip", "previous_cpu_billboard_vertices_and_lerp_factor"],
        "deferred_particle": ["previous_object_to_clip", "previous_particle_vertices_or_instances", "alpha_coverage_matching"],
        "forward_particle": ["previous_object_to_clip", "previous_particle_vertices_or_instances", "transparent_coverage_policy"],
        "water": ["previous_object_to_clip", "previous_watermove_time_and_parameters", "transparent_coverage_policy"],
        "sky": ["previous_rotation_projection", "sky_coverage_without_geometry_depth"],
        "clouds": ["previous_cloud_geometry_and_scroll", "transparent_coverage_policy"],
        "tessellation": ["previous_surface_after_domain_displacement", "previous_patch_positions_normals_and_view", "consistent_current_previous_barycentric_location"],
    }
    files = {item["path"]: item for item in corpus["files"]}
    cyclic = {path for cycle in corpus["include_cycles"] for path in cycle}
    candidates = []
    unclassified = []
    excluded = []
    for item in corpus["files"]:
        if item["stage"] not in ("vs", "hs", "ds", "gs"):
            continue
        path = item["path"]
        label = item["family"]
        if label == "shadow_only" or path == "dx11/tess_shadow.ds":
            excluded.append({"path": path, "reason": "shadow_map_not_scene_motion"})
            continue
        if label not in requirements:
            unclassified.append({"path": path, "family": label, "status": "requires_pass_role_review_not_covered"})
            continue
        closure = item["dependency_closure"]
        modes = []
        # Inspect the entrypoint owner, not skin.h helper definitions.
        for entry in item["entrypoints"]:
            for field in entry["inputs"]:
                struct = field["struct"]
                if struct == "v_model":
                    modes.append("SKIN_NONE")
                elif re.fullmatch(r"v_model_skinned_[0-4]", struct):
                    modes.append("SKIN_" + struct[-1])
        ordered_modes = [mode for mode in ("SKIN_NONE", "SKIN_0", "SKIN_1", "SKIN_2", "SKIN_3", "SKIN_4") if mode in modes]
        candidates.append({"path": path, "sha256": item["sha256"], "stage": item["stage"],
                           "family": label, "previous_state": requirements[label],
                           "skin_modes": ordered_modes,
                           "source_defines": sorted({define for name in closure for define in files[name]["defines"]}),
                           "conditional_axes": sorted({directive["directive"] for name in closure for directive in files[name]["conditional_directives"]}),
                           "entrypoint_owners": sorted({entry["source"] for entry in item["entrypoints"]}),
                           "status": "blocked_include_cycle" if cyclic.intersection(closure) else "planned_not_implemented"})
    return {"schema": 1, "runtime_ready": False, "corpus_sha256": corpus["corpus_sha256"],
            "file_count": corpus["file_count"],
            "scope": "source_wrapper_and_compiler_axes_not_live_permutation_census",
            "geometry_candidates": candidates, "unclassified_stage_roots": unclassified,
            "excluded_stage_roots": excluded,
            "colour_contract": {
                "pre_tonemap_local_value": "combine_1.ps:144-150 color before tonemap",
                "complete_stock_fp16_scene_texture_available": False,
                "generic_targets": "LDR8; not a recoverable linear radiance input",
                "accumulator": "FP16 lighting only, not the complete scene",
                "additional_colour_writers_to_handle": ["sky2.vs:28-29", "sky2.ps:24-32", "clouds.vs:28-38", "combine_volumetric.ps:29-45", "forward materials after combine_1"],
                "later_colour_consumers": ["bloom", "combine_2_naa.ps:63-75 optional second curve", "combine_2_aa.ps:67-76 different AA path"],
                "engine_requirements": ["preserve sky/fog alpha blending", "do not substitute a scaled accumulator", "update fixed UNORM resolves/copy descriptors if changing target formats", "retain native exposure/bloom and downstream colour contract"],
                "evidence": ["common_functions.h:21-29", "r4_rendertarget.cpp:431-444,464", "blender_combine.cpp:19-20", "r4_rendertarget_phase_combine.cpp:83-110,284-324"]},
            "depth_contract": {
                "view_z_channel": "s_position.z", "units": "linear view-space game units, not normalized device depth",
                "target0_xy": "packed normal; INTERNAL_AMD changes the encoding",
                "target0_w": "encoded hemi/material, not homogeneous W",
                "target1": "albedo/specular", "position_reconstruction": "view XY from pixel position, view Z and pos_decompression_params",
                "virtual_height_bias": "some pixel writers offset interpolated position along normal before packing",
                "default_sky": "position clear zero; SKY_WITH_DEPTH is not enabled by stock common_defines.h",
                "hud": "separate projection and rmNear hardware viewport depth 0..0.02 (r4.cpp:834-840); retain view identity",
                "evidence": ["gbuffer_stage.h:5-11,104-128", "deffer_base_flat.ps:34-39", "common_defines.h:9,21-23", "r4_rendertarget_phase_scene.cpp:14-22", "r__dsgraph_render.cpp:502-535"]},
            "skin_contract": {
                "SKIN_NONE": "Rigid vertices need prior object transform; software-skinned vertices additionally need prior CPU deformation. Do not assume rigid from this macro.",
                "SKIN_0": "Single-bone RM_SINGLE folds bone pose into the draw world transform. Preserve previous composed world, not just actor world.",
                "SKIN_1_to_4": "Previous sbones_array rows with the exact current index/weight packing; palette is 234 float4 rows, not 234 matrices.",
                "evidence": ["skin.h:6-58,83-247", "SkeletonX.cpp:60-98,196-241"]},
            "tessellation_axes": [["TESS_PN"], ["TESS_HM"], ["TESS_PN", "TESS_HM"]],
            "required_preserved_axes": ["USE_LM_HEMI", "USE_TDETAIL", "USE_TDETAIL_BUMP", "USE_PARALLAX", "USE_STEEPPARALLAX", "USE_R2_STATIC_SUN", "USE_MSAA", "MSAA_SAMPLES", "MSAA_OPTIMIZATION", "ATOC", "MSAA_ALPHATEST_DX10_0_ATOC", "MSAA_ALPHATEST_DX10_1_ATOC", "MSAA_ALPHATEST_DX10_1", "EXTEND_F_DEFFER"],
            "motion_mrt": {"status": "not_provided_requires_shader_and_engine_changes",
                "existing_scene_outputs": ["SV_Target0 packed normal/view-z/hemi/material", "SV_Target1 albedo/specular"],
                "proposed_output": "SV_Target2 motion plus a separately specified validity/coverage contract",
                "engine_requirements": ["allocate compatible dimensions/sample count", "bind RTV2 in phase_scene_begin and preserve it for all relevant writes", "clear invalid coverage each view/frame", "update v2p/p/HS/DS/PS interfaces together", "preserve alpha-test/sample coverage", "resolve motion with depth/coverage policy, not blind averaging", "sky and forward transparent writers need explicit coverage and composition"],
                "evidence": ["gbuffer_stage.h:5-11,95-128", "r4_rendertarget_phase_scene.cpp:31-55"]},
            "reflection_cache_requirements": ["Replace bytecode coherently before engine create_shader reflection and VS input-signature extraction", "Handle cache-hit and source-compile paths, not only D3DCompile or CreateVertexShader", "Namespace cache by original corpus, patch ABI, macros, target, flags, compiler and first-party include", "Bind previous state by stable draw/object/skeleton/detail-instance AND view identity, preserving reflected layout and matrix convention", "Keep first-person, main, scope, cut and invalid-history identities distinct"],
            "pda": {"material": "pda_overlay.s", "vertex_shader": "model_def_lplanes.vs", "pixel_shader": "pda_overlay.ps", "texture": "$user$ui", "sampler": "s_pdascreen", "sampling_state": "smp_base", "depth_test": True, "depth_write": False, "blend": "srcalpha/invsrcalpha", "distortion": True, "surface_motion_and_uv_content_animation_are_distinct": True}}


REPO = Path(__file__).resolve().parents[1]
LOCAL_ROOT = REPO / ".local/anomaly"
STAGING_ROOT = LOCAL_ROOT / "staging"


def bounded_output(path, base):
    path = Path(path).absolute()
    for component in (path, *path.parents):
        if component.is_symlink() or component.is_junction():
            raise Refusal(f"linked output path not allowed: {component}")
    resolved = path.resolve()
    base = Path(base).resolve()
    if resolved == base or not resolved.is_relative_to(base):
        raise Refusal(f"output must be a child of {base}")
    return resolved


def stage(source, output, profile, injected):
    """Preflight all inputs, then write a new local preview with rollback on error."""
    import shutil
    output = bounded_output(output, STAGING_ROOT)
    source = Path(source).resolve(strict=True)
    if output.exists():
        raise Refusal("staging destination already exists")
    if output.is_relative_to(source) or source.is_relative_to(output):
        raise Refusal("source and staging must not overlap")
    blobs = read_tree(source)
    try:
        if profile["schema"] != 1:
            raise Refusal("unsupported profile schema")
        if len(blobs) != profile["file_count"] or tree_digest(blobs) != profile["corpus_sha256"]:
            raise Refusal("entire original corpus hash/count mismatch")
        rule = profile["patch"]
        name = rule["path"]
        if name != PurePosixPath(name).as_posix() or ".." in PurePosixPath(name).parts or name not in blobs:
            raise Refusal("patch target is not an original asset path")
        header = profile["include"]["name"]
        if digest(injected) != profile["include"]["sha256"]:
            raise Refusal("first-party include hash mismatch")
        if header.casefold() in {key.casefold() for key in blobs}:
            raise Refusal("first-party include collides with original asset")
        originals = {key: digest(value) for key, value in blobs.items()}
        blobs[name] = inject_include(blobs[name], rule, header)
        blobs[header] = injected
    except (KeyError, TypeError) as error:
        raise Refusal(f"malformed profile: {error}") from error
    report = {"schema": 1, "operation": "declarations_only_include_preview",
              "runtime_ready": False, "motion_output_implemented": False,
              "compiled": False, "source_corpus_sha256": profile["corpus_sha256"],
              "source_file_count": len(originals), "staged_file_count": len(blobs),
              "total_files_including_manifest": len(blobs) + 1,
              "files": [{"path": key, "original_sha256": originals.get(key),
                         "staged_sha256": digest(value)} for key, value in sorted(blobs.items())]}
    output.parent.mkdir(parents=True, exist_ok=True)
    output.mkdir(exist_ok=False)
    try:
        for key, value in blobs.items():
            target = output / key
            target.parent.mkdir(parents=True, exist_ok=True)
            with target.open("xb") as stream:
                stream.write(value)
        if read_tree(output) != blobs:
            raise Refusal("staged readback differs from expected bytes")
        with (output / ".rsf-stage.json").open("x", encoding="utf-8", newline="\n") as stream:
            json.dump(report, stream, indent=2)
            stream.write("\n")
    except BaseException:
        shutil.rmtree(output)
        raise
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    for command in ("inventory", "plan", "stage-include"):
        operation = sub.add_parser(command)
        operation.add_argument("--source", type=Path, required=True)
        operation.add_argument("--output", type=Path, required=True)
        if command == "stage-include":
            operation.add_argument("--profile", type=Path,
                                   default=Path(__file__).with_name("anomaly_shader_vanilla_profile.json"))
    args = parser.parse_args()
    try:
        if args.command == "stage-include":
            profile = json.loads(args.profile.read_text(encoding="utf-8"))
            include = Path(__file__).with_name("anomaly_shader_constants.h").read_bytes()
            result = stage(args.source, args.output, profile, include)
            print(json.dumps({key: value for key, value in result.items() if key != "files"}))
        else:
            output = bounded_output(args.output, LOCAL_ROOT)
            source = args.source.resolve(strict=True)
            if output.is_relative_to(source):
                raise Refusal("inventory output must not be placed inside source assets")
            if output.exists():
                raise Refusal("inventory output already exists")
            result = inventory(source)
            if args.command == "plan":
                result = patch_plan(result)
            output.parent.mkdir(parents=True, exist_ok=True)
            with output.open("x", encoding="utf-8", newline="\n") as stream:
                json.dump(result, stream, indent=2)
                stream.write("\n")
            print(json.dumps({"file_count": result["file_count"], "corpus_sha256": result["corpus_sha256"]}))
    except (Refusal, OSError, ValueError) as error:
        parser.exit(2, f"refused: {error}\n")


if __name__ == "__main__":
    main()

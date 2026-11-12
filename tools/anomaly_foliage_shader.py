#!/usr/bin/env python3
"""Bounded real-asset foliage transformations, never a deployment tool.

Only identifier substitutions and first-party wrappers are tracked. Original
shader expressions are obtained from the caller's pinned licensed local corpus.
Shared v2p / pixel augmentation belongs to the independent core worker.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil

REPO = Path(__file__).resolve().parents[1]
CORPUS_SHA256 = "fdc260e0608f259d876c4ae0f11a6f2d8c03c001cf1fe82548794af1c6826a67"
OWNERS = ("deffer_tree_flat.vs", "deffer_tree_bump.vs",
          "deffer_tree_s_flat.vs", "deffer_tree_s_bump.vs",
          "deffer_detail_w_flat.vs", "deffer_detail_s_flat.vs")
HEADER = REPO / "games/anomaly/shaders/foliage/rsf_anomaly_foliage.h"


class Refusal(ValueError):
    """The input is not the reviewed corpus or operation."""


def entrypoint(text, name="main"):
    pattern = (r"\b(v2p_flat|v2p_bumped)\s+" + re.escape(name) +
               r"\s*\(\s*(v_tree|v_detail)\s+(\w+)\s*\)\s*\{")
    matches = list(re.finditer(pattern, text))
    if len(matches) != 1:
        raise Refusal(f"expected one foliage entrypoint: {name}")
    match = matches[0]
    depth = 1
    end = match.end()
    while end < len(text) and depth:
        depth += (text[end] == "{") - (text[end] == "}")
        end += 1
    if depth:
        raise Refusal("unbalanced foliage entrypoint")
    return {"start": match.start(), "end": end,
            "return_type": match[1], "input_type": match[2], "input": match[3],
            "body": text[match.end() - 1:end]}


def _read_corpus(source):
    source = Path(source).absolute()
    for parent in (source, *source.parents):
        if parent.is_symlink() or parent.is_junction():
            raise Refusal("linked source")
    blobs = {}
    for path in sorted(source.rglob("*")):
        if path.is_symlink() or path.is_junction():
            raise Refusal("linked source")
        if path.is_file():
            blobs[path.relative_to(source).as_posix()] = path.read_bytes()
    rows = [[key, hashlib.sha256(value).hexdigest()] for key, value in sorted(blobs.items())]
    sha = hashlib.sha256(json.dumps(rows, separators=(",", ":"), ensure_ascii=True).encode()).hexdigest()
    if len(blobs) != 449 or sha != CORPUS_SHA256:
        raise Refusal("foliage transformation requires pinned vanilla corpus")
    return blobs, sha


def transform(source):
    return _transform(*_read_corpus(source))


def _transform(blobs, sha):
    patches = {HEADER.name: HEADER.read_bytes()}
    for name in OWNERS:
        text = blobs[name].decode("utf-8-sig")
        entry = entrypoint(text)
        body = entry["body"]
        remap = {"consts": "rsf_previous_consts", "wave": "rsf_previous_wave",
                 "wind": "rsf_previous_wind", "dir2D": "rsf_previous_wind",
                 "array": "rsf_previous_rows", "m_xform": "rsf_previous_tree_xform",
                 "m_VP": "rsf_previous_clip_from_world", "m_WVP": "rsf_previous_clip_from_world"}
        previous = re.sub(r"\b(" + "|".join(remap) + r")\b", lambda m: remap[m[0]], body)
        signature = f'{entry["return_type"]} {{name}}({entry["input_type"]} {entry["input"]})\n'
        current = signature.format(name="rsf_foliage_current") + body
        prior = signature.format(name="rsf_foliage_previous") + previous
        variable = entry["input"]
        validity = "rsf_tree_history_valid()"
        if entry["input_type"] == "v_detail":
            validity = f"rsf_detail_history_valid({variable}.misc.w)"
        wrapper = signature.format(name="main") + f"""{{
    {entry['return_type']} rsf_out = rsf_foliage_current({variable});
    rsf_out.rsf_previous_clip = rsf_out.hpos;
    rsf_out.rsf_valid = 0;
    if ({validity})
    {{
        rsf_out.rsf_previous_clip = rsf_foliage_previous({variable}).hpos;
        rsf_out.rsf_valid = 1;
    }}
    return rsf_out;
}}
"""
        output = ('#define RSF_TEMPORAL 1\n' + text[:entry["start"]] + '#include "rsf_anomaly_foliage.h"\n' + current +
                  "\n\n" + prior + "\n\n" + wrapper + text[entry["end"]:])
        patches[name] = output.encode()
    return patches, {"schema": 1, "runtime_ready": False, "source_corpus_sha256": sha,
                     "blocked_roots": ["deffer_tree_s_bump_d.vs"],
                     "shared_core_required": True, "vs_constant_slot": 13,
                     "previous_clip": "unjittered world-to-clip, column-vector multiply; row-major storage",
                     "motion_mrt": "SV_Target2 RGBA16_FLOAT: current-minus-previous UV Y-down, valid, 0"}


def stage(source, destination):
    """Stage local-only variants. Shared core interface is not implicitly patched."""
    source, destination = Path(source), Path(destination)
    permitted = (REPO / ".local/anomaly/staging").resolve()
    if not destination.resolve().is_relative_to(permitted) or destination.exists():
        raise Refusal("new destination under .local/anomaly/staging required")
    if destination.resolve().is_relative_to(source.resolve()):
        raise Refusal("destination must not be inside source")
    for parent in (destination, *destination.parents):
        if parent.is_symlink() or (parent.exists() and parent.is_junction()):
            raise Refusal("linked destination")
    originals, sha = _read_corpus(source)
    patches, report = _transform(originals, sha)
    report["abi"] = 1
    report["previous_valid_member"] = "rsf_valid"
    report["tool_sha256"] = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
    report["changed_files"] = [{"path": name, "sha256": hashlib.sha256(payload).hexdigest()}
                               for name, payload in sorted(patches.items())]
    report["cache_namespace"] = hashlib.sha256(
        json.dumps(report, sort_keys=True, separators=(",", ":")).encode()).hexdigest()
    created = False
    try:
        destination.mkdir(parents=True)
        created = True
        shutil.copytree(source, destination, dirs_exist_ok=True)
        for name, payload in patches.items():
            (destination / name).write_bytes(payload)
        expected_tree = {**originals, **patches}
        actual_names = set()
        for path in destination.rglob("*"):
            if path.is_symlink() or path.is_junction():
                raise Refusal("linked staged asset")
            if not path.is_file():
                continue
            name = path.relative_to(destination).as_posix()
            actual_names.add(name)
            if name not in expected_tree or path.read_bytes() != expected_tree[name]:
                raise Refusal(f"stage readback mismatch: {name}")
        if actual_names != set(expected_tree):
            raise Refusal("stage readback file-set mismatch")
        (destination / ".rsf-foliage-stage.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
        return report
    except BaseException:
        if created and destination.exists():
            shutil.rmtree(destination)
        raise


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        print(json.dumps(stage(args.source, args.output), indent=2))
    except (Refusal, OSError) as error:
        parser.exit(2, str(error) + "\n")

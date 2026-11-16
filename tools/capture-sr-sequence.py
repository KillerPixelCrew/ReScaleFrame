#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
"""Capture and classify four ready SR sources in an explicitly selected live Runtime."""
import argparse
import importlib.util
import json
import re
import time
from pathlib import Path


def load(name, filename):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(filename))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pid", type=int, required=True)
    parser.add_argument("--module-base", type=lambda value: int(value, 0), required=True)
    parser.add_argument("--runtime", type=Path, required=True)
    parser.add_argument("--sha256", required=True)
    parser.add_argument("--captures", type=Path, required=True)
    parser.add_argument("--report", type=Path, required=True)
    parser.add_argument("--timeout", type=float, default=120)
    parser.add_argument("--require-fg", action="store_true", help="Wait for actual active FG before arming")
    args = parser.parse_args()
    if args.pid <= 0 or args.module_base <= 0 or not 1 <= args.timeout <= 600:
        parser.error("positive PID/base and timeout 1..600 seconds required")
    api = load("rsf_runtime_action", "runtime-action.py")
    analyzer = load("rsf_capture_analyzer", "analyze-sr-captures.py")
    remote = None
    report = {"pid": args.pid, "runtime_sha256": args.sha256, "status": "waiting", "require_fg": args.require_fg, "captures": []}
    started = time.monotonic()
    existing = set(args.captures.glob("*/manifest.json"))
    try:
        remote = api.Remote(args.pid, args.module_base, api.Image(str(args.runtime), args.sha256))
        stable = 0
        previous = None
        while time.monotonic() - started < args.timeout:
            status = remote.status()
            identity = (status.sr_effective_backend, status.render_width, status.render_height,
                        status.output_width, status.output_height)
            ready = bool(status.foreground and status.sr_enabled and status.sr_result == 0 and
                         all(identity) and (not args.require_fg or
                         (status.fg_active and status.fg_effective_backend and status.output_fps_valid)))
            stable = stable + 1 if ready and identity == previous else int(ready)
            previous = identity
            if stable >= 3:
                report["ready_status"] = api.plain(status)
                break
            time.sleep(0.5)
        else:
            raise TimeoutError("No stable foreground SR state within deadline")
        remote.write(512, api.struct.pack("<4I", 16, 1, 1, 1))
        remote._call(remote.allow_export("rsf_d3d11_host_capture_control"), remote.memory + 512)
        report["status"] = "capturing"
        print("Four-frame sequence armed", flush=True)
        while time.monotonic() - started < args.timeout:
            rows = []
            for manifest_path in sorted(args.captures.glob("*/manifest.json")):
                bundle = manifest_path.parent
                if manifest_path in existing or not re.search(rf"-{args.pid}-[0-9]+$", bundle.name):
                    continue
                if not (bundle / "sequence_capture_status.json").is_file():
                    continue
                try:
                    manifest = json.loads(manifest_path.read_text(encoding="utf-8-sig"))
                except (OSError, ValueError):
                    continue
                if manifest.get("status") not in ("complete", "partial", "cancelled", "incomplete_waiting_for_gpu_or_frame"):
                    continue
                rows.append(analyzer.inspect(bundle))
            report["captures"] = rows
            if len(rows) >= 4:
                report["status"] = "captured"
                return 0 if all(row["classification"] == "evaluated-presented" for row in rows) else 1
            remote.status()  # A dead selected process is an error, not an empty capture set.
            time.sleep(0.5)
        raise TimeoutError("Capture sequence did not finish within deadline")
    except Exception as error:
        report["status"] = "failed"
        report["error"] = str(error)
        return 2
    finally:
        if remote is not None:
            remote.close()
        report["elapsed_seconds"] = time.monotonic() - started
        report["visual_acceptance"] = "not-established"
        args.report.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
        print(json.dumps(report, indent=2))


if __name__ == "__main__":
    raise SystemExit(main())

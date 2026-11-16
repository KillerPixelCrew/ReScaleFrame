#!/usr/bin/env python3
"""Bounded, opt-in CPU stall observation for an already running RSF trial.

No GPU captures, settings changes, input, process launch, or process termination.
The explicitly supplied Windows debugger attaches noninvasively once on a stall.
"""
import argparse
import importlib.util
import json
from pathlib import Path
import subprocess
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('pid', type=int)
    parser.add_argument('module_base', type=lambda value: int(value, 0))
    parser.add_argument('runtime', type=Path)
    parser.add_argument('sha256')
    parser.add_argument('--debugger', type=Path, required=True)
    parser.add_argument('--report', type=Path, required=True)
    parser.add_argument('--seconds', type=float, default=120)
    parser.add_argument('--stall-seconds', type=float, default=3)
    args = parser.parse_args()
    if not 1 <= args.stall_seconds <= 30 or not 1 <= args.seconds <= 600:
        parser.error('Observation must be bounded to 600 seconds and stall threshold to 1..30 seconds')
    if not args.debugger.is_file():
        parser.error('Explicit debugger executable is missing')
    args.report.mkdir(parents=True, exist_ok=False)
    spec = importlib.util.spec_from_file_location('runtime_action', Path(__file__).with_name('runtime-action.py'))
    api = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(api)
    remote = None
    report = {'pid': args.pid, 'gpu_capture': False, 'samples': [], 'outcome': 'incomplete'}
    try:
        remote = api.Remote(args.pid, args.module_base, api.Image(args.runtime, args.sha256))
        started = changed = time.monotonic()
        previous = None
        while time.monotonic() - started < args.seconds:
            status = api.plain(remote.status())
            now = time.monotonic()
            report['samples'].append({'elapsed_seconds': now - started, 'status': status})
            identity = (status['application_presents'], status['fg_effective_backend'])
            # Fallout suspends its render loop while unfocused. A retained Active
            # snapshot is not progress and must not trigger a debugger attachment.
            if identity != previous or not status['foreground'] or not status['fg_requested'] or not status['source_frame']:
                changed = now
            previous = identity
            if now - changed >= args.stall_seconds:
                # A live, hash-checked Remote status query just succeeded. The debugger
                # is read-only (-pv) and q leaves the target running.
                dump = (args.report / 'stall.dmp').resolve()
                symbols = args.runtime.resolve().parent
                command = f'.sympath "{symbols}"; ~* kp; .dump /m "{dump}"; q'
                with (args.report / 'threads.txt').open('w', encoding='utf-8') as output:
                    result = subprocess.run([str(args.debugger), '-pv', '-p', str(args.pid), '-c', command],
                                            stdout=output, stderr=subprocess.STDOUT, timeout=45,
                                            creationflags=subprocess.CREATE_NO_WINDOW, check=False)
                report.update(outcome='stall_observed', debugger_exit=result.returncode,
                              dump_created=dump.is_file() and dump.stat().st_size > 0)
                break
            time.sleep(0.5)
        else:
            report['outcome'] = 'observation_finished_without_stall'
    except Exception as error:
        report.update(outcome='observation_error', error=str(error))
    finally:
        if remote is not None:
            try:
                remote.close()
            except Exception as error:
                report['cleanup_error'] = str(error)
        (args.report / 'report.json').write_text(json.dumps(report, indent=2), encoding='utf-8')
    print(json.dumps({key: value for key, value in report.items() if key != 'samples'}, indent=2))
    return 0 if report['outcome'] != 'observation_error' else 1


if __name__ == '__main__':
    raise SystemExit(main())

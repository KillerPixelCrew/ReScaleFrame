#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
"""Bounded foreground-only relative mouse motion with sampled RSF status evidence."""
import argparse
import ctypes as c
from ctypes import wintypes as w
import importlib.util
import json
from pathlib import Path
import time


def relative_sender(pid):
    class Mouse(c.Structure):
        _fields_=[("dx",w.LONG),("dy",w.LONG),("data",w.DWORD),("flags",w.DWORD),("time",w.DWORD),("extra",c.c_size_t)]
    class Data(c.Union):
        _fields_=[("mouse",Mouse),("padding",c.c_byte*32)]
    class Input(c.Structure):
        _fields_=[("kind",w.DWORD),("data",Data)]
    user=c.WinDLL("user32",use_last_error=True)
    user.GetForegroundWindow.restype=w.HWND
    user.GetWindowThreadProcessId.argtypes=[w.HWND,c.POINTER(w.DWORD)]
    user.SendInput.argtypes=[w.UINT,c.POINTER(Input),c.c_int]
    def send(dx,dy):
        actual=w.DWORD()
        user.GetWindowThreadProcessId(user.GetForegroundWindow(),c.byref(actual))
        if actual.value!=pid:
            raise RuntimeError("Selected process lost foreground; no further mouse packets sent")
        event=Input();event.kind=0;event.data.mouse=Mouse(dx,dy,0,1,0,0)
        if user.SendInput(1,c.byref(event),c.sizeof(event))!=1:
            raise c.WinError(c.get_last_error())
    return send


def run(send,status,steps,dx,dy,interval,rows):
    rows.append({"phase":"before","status":status()})
    for step in range(steps):
        send(dx,dy)
        if step%4==0:
            rows.append({"phase":"pan","step":step,"status":status()})
        time.sleep(interval)
    rows.append({"phase":"after","status":status()})


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pid",type=int,required=True)
    parser.add_argument("--module-base",type=lambda value:int(value,0),required=True)
    parser.add_argument("--runtime",required=True)
    parser.add_argument("--sha256",required=True)
    parser.add_argument("--report",type=Path,required=True)
    parser.add_argument("--dx",type=int,default=2)
    parser.add_argument("--dy",type=int,default=0)
    parser.add_argument("--steps",type=int,default=120)
    args=parser.parse_args()
    if args.pid<=0 or args.module_base<=0 or not 1<=args.steps<=240 or max(abs(args.dx),abs(args.dy))>10:
        parser.error("positive PID/base, 1..240 steps and deltas within +/-10 required")
    spec=importlib.util.spec_from_file_location("rsf_actions",Path(__file__).with_name("runtime-action.py"))
    api=importlib.util.module_from_spec(spec);spec.loader.exec_module(api)
    remote=None;rows=[];report={"pid":args.pid,"dx":args.dx,"dy":args.dy,"steps":args.steps,"samples":rows}
    try:
        remote=api.Remote(args.pid,args.module_base,api.Image(args.runtime,args.sha256))
        send=relative_sender(args.pid)
        run(send,lambda:api.plain(remote.status()),args.steps,args.dx,args.dy,1/60,rows)
        report["status"]="input-complete"
        return 0
    except Exception as error:
        report["status"]="failed";report["error"]=str(error)
        return 1
    finally:
        if remote is not None:remote.close()
        report["motion_verified"]=False
        report["limits"]="Input delivery and sampled status only. Verify camera movement from captures; sampling can miss single-frame transitions."
        args.report.write_text(json.dumps(report,indent=2)+"\n",encoding="utf-8")
        print(json.dumps(report,indent=2))


if __name__=="__main__":
    raise SystemExit(main())

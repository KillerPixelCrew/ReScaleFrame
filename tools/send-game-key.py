#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Send one held scan-code key only to an explicitly selected foreground process."""
import argparse
import ctypes as c
from ctypes import wintypes as w
import json
import time


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pid",type=int,required=True)
    parser.add_argument("--key",choices=("Enter","Escape","Up","Down","Left","Right"),required=True)
    args=parser.parse_args()
    if args.pid<=0: parser.error("positive PID required")
    class Mouse(c.Structure):
        _fields_=[("dx",w.LONG),("dy",w.LONG),("data",w.DWORD),("flags",w.DWORD),("time",w.DWORD),("extra",c.c_size_t)]
    class Keyboard(c.Structure):
        _fields_=[("vk",w.WORD),("scan",w.WORD),("flags",w.DWORD),("time",w.DWORD),("extra",c.c_size_t)]
    class Data(c.Union):
        _fields_=[("mouse",Mouse),("keyboard",Keyboard)]
    class Input(c.Structure):
        _fields_=[("kind",w.DWORD),("data",Data)]
    user=c.WinDLL("user32",use_last_error=True)
    user.GetForegroundWindow.restype=w.HWND
    user.GetWindowThreadProcessId.argtypes=[w.HWND,c.POINTER(w.DWORD)]
    user.SendInput.argtypes=[w.UINT,c.POINTER(Input),c.c_int]
    foreground=w.DWORD()
    user.GetWindowThreadProcessId(user.GetForegroundWindow(),c.byref(foreground))
    if foreground.value!=args.pid: raise RuntimeError("Selected process is not foreground; nothing sent")
    scans={"Enter":0x1c,"Escape":1,"Up":0x48,"Down":0x50,"Left":0x4b,"Right":0x4d}
    flags=8 | (1 if args.key in ("Up","Down","Left","Right") else 0)
    event=Input();event.kind=1;event.data.keyboard=Keyboard(0,scans[args.key],flags,0,0)
    if user.SendInput(1,c.byref(event),c.sizeof(event))!=1: raise c.WinError(c.get_last_error())
    try:
        time.sleep(0.12)
    finally:
        event.data.keyboard.flags|=2
        if user.SendInput(1,c.byref(event),c.sizeof(event))!=1: raise c.WinError(c.get_last_error())
    print(json.dumps({"pid":args.pid,"key":args.key,"held_ms":120,"sent":True,"effect_verified":False}))


if __name__=="__main__":
    main()

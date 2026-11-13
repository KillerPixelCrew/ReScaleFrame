// SPDX-License-Identifier: GPL-3.0-only
using System;
using System.Runtime.InteropServices;
using ReScaleFrame;
internal static class Program {
    [DllImport("ReScaleFrame.Unity.Native.dll",CallingConvention=CallingConvention.Cdecl)]
    private static extern int rsfu_settings(ref Native.Settings value);
    private static void Require(bool value,string reason) { if(!value)throw new InvalidOperationException(reason); }
    private static int Main(string[] args) {
        if(args.Length!=1)return 1;
        var module=NativeLibrary.Load(args[0]);
        NativeLibrary.SetDllImportResolver(typeof(Program).Assembly,(name,assembly,path)=>name=="ReScaleFrame.Unity.Native.dll"?module:IntPtr.Zero);
        Require(Marshal.SizeOf<Native.Settings>()==100&&Marshal.SizeOf<Native.Sharpening>()==56,"settings ABI sizes");
        Require(Marshal.SizeOf<Native.Status>()==184&&Marshal.SizeOf<Native.Api>()==72&&Marshal.SizeOf<Native.CpuFact>()==64,"latency status and Game CPU fact ABI sizes");
        Require(Marshal.OffsetOf<Native.Settings>(nameof(Native.Settings.Sharpen)).ToInt32()==44,"settings append offset");
        Require(Marshal.OffsetOf<Native.Sharpening>(nameof(Native.Sharpening.mode)).ToInt32()==8,"mode ABI offset");
        Require(Marshal.OffsetOf<Native.Sharpening>(nameof(Native.Sharpening.sharpness)).ToInt32()==12,"sharpness ABI offset");
        Require(Marshal.OffsetOf<Native.Sharpening>(nameof(Native.Sharpening.mas)).ToInt32()==16,"mas ABI offset");
        Require(Marshal.OffsetOf<Native.Sharpening>(nameof(Native.Sharpening.motion_sharpness)).ToInt32()==20,"motion_sharpness ABI offset");
        Require(Marshal.OffsetOf<Native.Sharpening>(nameof(Native.Sharpening.motion_threshold)).ToInt32()==24,"motion_threshold ABI offset");
        Require(Marshal.OffsetOf<Native.Sharpening>(nameof(Native.Sharpening.motion_scale_limit)).ToInt32()==28,"motion_scale_limit ABI offset");
        Require(Marshal.OffsetOf<Native.Sharpening>(nameof(Native.Sharpening.contrast_enabled)).ToInt32()==32,"contrast_enabled ABI offset");
        Require(Marshal.OffsetOf<Native.Sharpening>(nameof(Native.Sharpening.contrast)).ToInt32()==36,"contrast ABI offset");
        Require(Marshal.OffsetOf<Native.Sharpening>(nameof(Native.Sharpening.depth_scale)).ToInt32()==40,"depth_scale ABI offset");
        Require(Marshal.OffsetOf<Native.Sharpening>(nameof(Native.Sharpening.depth_bias)).ToInt32()==44,"depth_bias ABI offset");
        Require(Marshal.OffsetOf<Native.Sharpening>(nameof(Native.Sharpening.clamp_output)).ToInt32()==48,"clamp_output ABI offset");
        Require(Marshal.OffsetOf<Native.Sharpening>(nameof(Native.Sharpening.debug)).ToInt32()==52,"debug ABI offset");
        var value=new Native.Settings {Size=100,Version=2,Sr=7,Quality=1,Fg=7,Generated=1,Gameplay=1,
            Sharpen=new Native.Sharpening {Size=56,Version=1,sharpness=.713f,mas=1,motion_sharpness=-.31f,
                motion_threshold=4,motion_scale_limit=19,contrast_enabled=1,contrast=1.6f,depth_scale=128,
                depth_bias=.0037f,clamp_output=1,debug=1}};
        for(uint mode=0;mode<=4;++mode) {
            value.Sharpen.mode=mode;Require(Native.rsfu_configure(ref value)==1,"live configure mode "+mode);
            var actual=new Native.Settings {Size=100,Version=2};Require(rsfu_settings(ref actual)==1&&actual.Equals(value),"all 12 fields cross the managed/native ABI");
        }
        var changed=value;changed.Sharpen.sharpness=.2f;Require(!changed.Equals(value),"nested live settings equality detects slider changes");
        changed=value;changed.Sharpen.mode=5;Require(Native.rsfu_configure(ref changed)==0,"native rejects invalid managed mode");
        Console.WriteLine("PASS: Unity package C# ABI size/all offsets, 12-field native round trip, five live modes and change detection");
        return 0;
    }
}

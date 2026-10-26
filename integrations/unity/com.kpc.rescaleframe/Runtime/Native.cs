// SPDX-License-Identifier: GPL-3.0-only
using System;
using System.Runtime.InteropServices;
using UnityEngine;

namespace ReScaleFrame
{
    internal static class Native
    {
        private const string Library = "ReScaleFrame.Unity.Native.dll";
        [StructLayout(LayoutKind.Sequential)] internal struct CameraFrame {
            internal uint Size, Version;
            internal Matrix4x4 Projection, InverseProjection, ViewToWorld, WorldToView, ClipToPrevious;
            internal Vector2 Jitter, PreviousJitter;
            internal uint RenderWidth, RenderHeight, OutputWidth, OutputHeight;
            internal float Near, Far, Fov;
            internal uint ReversedDepth, MotionJittered;
            internal float DeltaTime;
        }
        [StructLayout(LayoutKind.Sequential)] internal struct Packet {
            internal uint Size, Version;
            internal ulong Session, Frame, View;
            internal uint Generation, Flags;
            internal IntPtr Color, Depth, Motion, Output;
            internal CameraFrame Camera;
            internal Matrix4x4 PreviousToClip;
        }
        [StructLayout(LayoutKind.Sequential)] internal struct Configuration {
            internal uint Size, Version, Enabled, EngineSpatial, Quality, Generation;
            internal uint RenderWidth, RenderHeight, OutputWidth, OutputHeight;
        }
        [StructLayout(LayoutKind.Sequential)] internal struct Api {
            internal uint Size, Version;
            internal ulong Session;
            internal IntPtr Log, Config, Enqueue, RenderEvent, ManagedState, CpuEvent;
        }
        [StructLayout(LayoutKind.Sequential)] internal struct Settings {
            internal uint Size, Version, Sr, Quality, Preset, Fg, FgMode, Generated, Latency, LimitUs, Gameplay;
        }
        [StructLayout(LayoutKind.Sequential)] internal struct FrameCounters {
            internal uint Size, Version, Valid, Active;
            internal ulong TotalPresented, GeneratedPresented, ProviderGeneration;
        }
        [StructLayout(LayoutKind.Sequential)] internal struct Status {
            public uint Size, Version, RequestedSR, ActiveSR, RenderWidth, RenderHeight, OutputWidth, OutputHeight;
            public int SrResult, FgResult;
            public uint FgBackend, FgAvailable, FgActive, MaxGeneratedFrames, GeneratedFrames;
            public uint LatencyAvailable, EffectiveLatency, Reason;
            public ulong EvaluatedFrames, RefusedFrames, TaggedFrames;
            public ulong SleepCalls, MarkerCalls;
            public uint PclAvailable, LatencyReportAvailable;
        }
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] internal delegate int ConfigDelegate(uint width, uint height, ref Configuration config);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] internal delegate IntPtr EnqueueDelegate(ref Packet packet);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] internal delegate void StateDelegate(uint state);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] internal delegate void CpuDelegate(uint stage, ulong frame);
        [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int rsfu_initialize(
            [MarshalAs(UnmanagedType.LPUTF8Str)] string runtime, [MarshalAs(UnmanagedType.LPUTF8Str)] string version,
            [MarshalAs(UnmanagedType.LPUTF8Str)] string project, out Api api);
        [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int rsfu_configure(ref Settings settings);
        [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int rsfu_status(ref Status status);
        [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int rsfu_shutdown();
        [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int rsfu_input(ulong frame, uint kinds);
        [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int rsfu_temporal_ready();
        [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int rsfu_surface(uint width, uint height);
        [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int rsfu_preview_region(int left, int top, int width, int height, uint visible);
        [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int rsfu_frame_counters(ref FrameCounters counters);
        [DllImport("kernel32.dll", CharSet = CharSet.Unicode, ExactSpelling = true)] private static extern IntPtr GetModuleHandleW(string module);
        [DllImport("kernel32.dll", CharSet = CharSet.Ansi, ExactSpelling = true)] private static extern IntPtr GetProcAddress(IntPtr module, string name);
        internal static bool HasPreviewApi => GetProcAddress(GetModuleHandleW(Library), "rsfu_preview_region") != IntPtr.Zero;
        [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern IntPtr rsf_unity_get_render_event();
        [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int rsf_unity_get_event_id();
        internal static readonly uint SettingsSize = (uint)Marshal.SizeOf<Settings>();
        internal static readonly uint PacketSize = (uint)Marshal.SizeOf<Packet>();
        internal static readonly uint CameraSize = (uint)Marshal.SizeOf<CameraFrame>();
        internal static Api Bridge;
        internal static ConfigDelegate Config;
        internal static EnqueueDelegate Enqueue;
        internal static StateDelegate State;
        internal static CpuDelegate Cpu;
        internal static IntPtr RenderEvent;
        internal static int EventId;
        internal static int InitializationResult;
        internal static bool ProjectionReady;
        internal static ulong FrameId => (Bridge.Session << 32) | ((ulong)(uint)Time.frameCount + 1);
        internal static Configuration GetConfiguration(uint width, uint height) {
            var config = new Configuration { Size = (uint)Marshal.SizeOf<Configuration>(), Version = 2 };
            Config?.Invoke(width, height, ref config); ProjectionReady = config.Enabled != 0; return config;
        }
        internal static bool Start(string runtime, string project) {
            InitializationResult = rsfu_initialize(runtime, Application.unityVersion, project, out Bridge);
            if (InitializationResult != 1) return false;
            if (Bridge.Size != Marshal.SizeOf<Api>() || Bridge.Version != 4 || Bridge.Session == 0)
                throw new InvalidOperationException("ReScaleFrame native bridge ABI mismatch.");
            Config = Marshal.GetDelegateForFunctionPointer<ConfigDelegate>(Bridge.Config);
            Enqueue = Marshal.GetDelegateForFunctionPointer<EnqueueDelegate>(Bridge.Enqueue);
            State = Marshal.GetDelegateForFunctionPointer<StateDelegate>(Bridge.ManagedState);
            Cpu = Marshal.GetDelegateForFunctionPointer<CpuDelegate>(Bridge.CpuEvent);
            RenderEvent = rsf_unity_get_render_event(); EventId = rsf_unity_get_event_id();
            return RenderEvent != IntPtr.Zero;
        }
    }
}

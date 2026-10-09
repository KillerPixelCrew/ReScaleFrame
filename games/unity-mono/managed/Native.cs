// SPDX-License-Identifier: GPL-3.0-only
using System;
using System.Runtime.InteropServices;
using UnityEngine;

namespace ReScaleFrame.Unity
{
    [StructLayout(LayoutKind.Sequential)]
    internal struct CameraFrame
    {
        internal uint Size, Version;
        internal Matrix4x4 Projection, InverseProjection, ViewToWorld, WorldToView, ClipToPrevious;
        internal Vector2 Jitter, PreviousJitter;
        internal uint RenderWidth, RenderHeight, OutputWidth, OutputHeight;
        internal float Near, Far, Fov;
        internal uint ReversedDepth, MotionJittered;
        internal float DeltaTime;
    }

    [StructLayout(LayoutKind.Sequential)]
    internal struct Packet
    {
        internal uint Size, Version;
        internal ulong Session, Frame, View;
        internal uint Generation, Flags;
        internal IntPtr Color, Depth, Motion, Output;
        internal CameraFrame Camera;
        internal Matrix4x4 PreviousToClip;
    }

    // Mirrors RSF_UNITY_PACKET_* in unity_bridge.h.
    internal static class PacketFlags
    {
        internal const uint Reset = 1;       // history restarts at this frame
        internal const uint Probe = 2;       // camera constants only, no reconstruction requested
        internal const uint Window = 4;      // overlay event on the engine swapchain
        internal const uint Hudless = 8;     // completed scene colour before the UI draw
        internal const uint NoInputs = 16;   // depth or motion unusable, spatial fallback only
        internal const uint InputsOnly = 32; // depth and motion for generation, no reconstruction
    }

    [StructLayout(LayoutKind.Sequential)]
    internal struct Configuration
    {
        // EngineSpatial: the engine upscales spatially itself (no jitter, no history).
        internal uint Size, Version, Enabled, EngineSpatial, Quality, Generation;
        internal uint RenderWidth, RenderHeight, OutputWidth, OutputHeight;
    }

    [StructLayout(LayoutKind.Sequential)]
    internal struct Api
    {
        internal uint Size, Version;
        internal ulong Session;
        internal IntPtr Log, Config, Enqueue, RenderEvent, ManagedState, CpuEvent;
    }

    internal static class Native
    {
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] internal delegate void LogDelegate([MarshalAs(UnmanagedType.LPStr)] string message);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] internal delegate int ConfigDelegate(uint width, uint height, ref Configuration config);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] internal delegate IntPtr EnqueueDelegate(ref Packet packet);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] internal delegate void StateDelegate(uint state);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] internal delegate void CpuDelegate(uint stage, ulong frame);
        // RSF_UNITY_BRIDGE_ABI_VERSION and RSF_UNITY_NATIVE_ABI_VERSION.
        internal const uint BridgeVersion = 2, NativeVersion = 3;
        private static readonly int ApiSize = Marshal.SizeOf<Api>();
        internal static readonly uint PacketSize = (uint)Marshal.SizeOf<Packet>();
        internal static readonly uint CameraFrameSize = (uint)Marshal.SizeOf<CameraFrame>();
        private static readonly uint ConfigurationSize = (uint)Marshal.SizeOf<Configuration>();
        internal static Api Api;
        internal static LogDelegate Log;
        internal static ConfigDelegate GetConfig;
        internal static EnqueueDelegate Enqueue;
        internal static StateDelegate ReportState;
        internal static CpuDelegate ReportCpu;

        // Resolve through Unity's native-plugin loader so UnityPluginLoad receives its registry.
        [DllImport("ReScaleFrame.Game.UnityMono.dll", CallingConvention = CallingConvention.Cdecl)]
        internal static extern IntPtr rsf_unity_get_render_event();
        [DllImport("ReScaleFrame.Game.UnityMono.dll", CallingConvention = CallingConvention.Cdecl)]
        internal static extern int rsf_unity_get_event_id();

        internal static void Initialize(IntPtr address)
        {
            Api = Marshal.PtrToStructure<Api>(address);
            if (Api.Size != ApiSize || Api.Version != NativeVersion || Api.Session == 0 ||
                Api.Log == IntPtr.Zero || Api.Config == IntPtr.Zero || Api.Enqueue == IntPtr.Zero)
                throw new InvalidOperationException("Unity native bridge ABI mismatch.");
            Log = Marshal.GetDelegateForFunctionPointer<LogDelegate>(Api.Log);
            GetConfig = Marshal.GetDelegateForFunctionPointer<ConfigDelegate>(Api.Config);
            Enqueue = Marshal.GetDelegateForFunctionPointer<EnqueueDelegate>(Api.Enqueue);
            ReportState = Marshal.GetDelegateForFunctionPointer<StateDelegate>(Api.ManagedState);
            ReportCpu = Api.CpuEvent == IntPtr.Zero ? null : Marshal.GetDelegateForFunctionPointer<CpuDelegate>(Api.CpuEvent);
        }

        private static Configuration cached;
        private static int cachedFrame = -1;
        private static uint cachedWidth, cachedHeight;

        // Each frame several patched stages ask for the configuration. Sample it once per frame and
        // camera size, so every stage of a frame agrees and the P/Invoke and its native lock are paid once.
        internal static Configuration Configuration(Camera camera)
        {
            uint width = (uint)camera.pixelWidth, height = (uint)camera.pixelHeight;
            int frame = Time.frameCount;
            if (frame == cachedFrame && width == cachedWidth && height == cachedHeight) return cached;
            var config = new Configuration { Size = ConfigurationSize, Version = BridgeVersion };
            if (GetConfig(width, height, ref config) == 0)
                config.Enabled = 0;
            cached = config; cachedFrame = frame; cachedWidth = width; cachedHeight = height;
            return config;
        }

        internal static void ResetConfiguration() { cachedFrame = -1; }
    }
}

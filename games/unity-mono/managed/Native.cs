// SPDX-License-Identifier: GPL-3.0-only
using System;
using System.Runtime.InteropServices;
using UnityEngine;

namespace ReScaleFrame.Unity
{
    // Sequential x64 mirrors of unity_bridge.h and game_frame.h. Unity Matrix4x4 storage is copied
    // directly across the ABI; Snapshot composes transforms using Unity's matrix conventions.
    /// <summary>Camera transforms and dimensions; jitter uses render pixels, FOV radians and delta time seconds.</summary>
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

    /// <summary>Copied graph-execution packet; IntPtr fields name borrowed native D3D12 resources.</summary>
    /// <remarks>Flags are defined in unity_bridge.h. Enqueue takes resource leases and returns opaque event data.</remarks>
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

    /// <summary>Native policy snapshot, with exact input and output pixel dimensions.</summary>
    [StructLayout(LayoutKind.Sequential)]
    internal struct Configuration
    {
        internal uint Size, Version, Enabled, Backend, Quality, Generation;
        internal uint RenderWidth, RenderHeight, OutputWidth, OutputHeight;
    }

    /// <summary>Borrowed native function table retained through producer and GPU drainage.</summary>
    [StructLayout(LayoutKind.Sequential)]
    internal struct Api
    {
        internal uint Size, Version;
        internal ulong Session;
        internal IntPtr Log, Config, Enqueue, RenderEvent, ManagedState, CpuEvent;
    }

    /// <summary>Roots marshalled delegates and queries the active native session.</summary>
    internal static class Native
    {
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] internal delegate void LogDelegate([MarshalAs(UnmanagedType.LPStr)] string message);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] internal delegate int ConfigDelegate(uint width, uint height, ref Configuration config);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] internal delegate IntPtr EnqueueDelegate(ref Packet packet);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] internal delegate void StateDelegate(uint state);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] internal delegate void CpuDelegate(uint stage, ulong frame);
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

        /// <summary>Copy and validate the native table, then root delegates for the managed adapter lifetime.</summary>
        /// <exception cref="InvalidOperationException">Native size, version, session or required callbacks differ.</exception>
        internal static void Initialize(IntPtr address)
        {
            Api = Marshal.PtrToStructure<Api>(address);
            if (Api.Size != Marshal.SizeOf<Api>() || Api.Version != 2 || Api.Session == 0 ||
                Api.Log == IntPtr.Zero || Api.Config == IntPtr.Zero || Api.Enqueue == IntPtr.Zero)
                throw new InvalidOperationException("Unity native bridge ABI mismatch.");
            Log = Marshal.GetDelegateForFunctionPointer<LogDelegate>(Api.Log);
            GetConfig = Marshal.GetDelegateForFunctionPointer<ConfigDelegate>(Api.Config);
            Enqueue = Marshal.GetDelegateForFunctionPointer<EnqueueDelegate>(Api.Enqueue);
            ReportState = Marshal.GetDelegateForFunctionPointer<StateDelegate>(Api.ManagedState);
            ReportCpu = Api.CpuEvent == IntPtr.Zero ? null : Marshal.GetDelegateForFunctionPointer<CpuDelegate>(Api.CpuEvent);
        }

        /// <summary>Query policy for this camera's output size; refusal yields an inactive configuration.</summary>
        internal static Configuration Configuration(Camera camera)
        {
            var config = new Configuration { Size = (uint)Marshal.SizeOf<Configuration>(), Version = 1 };
            if (GetConfig((uint)camera.pixelWidth, (uint)camera.pixelHeight, ref config) == 0)
                config.Enabled = 0;
            return config;
        }
    }
}

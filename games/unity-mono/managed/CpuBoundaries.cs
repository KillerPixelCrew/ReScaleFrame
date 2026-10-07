// SPDX-License-Identifier: GPL-3.0-only
using System;
using System.Collections.Generic;
using UnityEngine;
using UnityEngine.LowLevel;
using UnityEngine.PlayerLoop;

namespace ReScaleFrame.Unity
{
    // Insert around existing player-loop phases without replacing any engine or mod delegate.
    // This covers input plus simulation before render submission; it does not claim individual
    // keyboard/controller device timing or an OS input-to-display latency measurement.
    /// <summary>Owns removable input/simulation markers in the existing Unity player loop.</summary>
    internal static class CpuBoundaries
    {
        private struct BeforeInput { }
        private struct AfterSimulation { }
        /// <summary>Insert markers before EarlyUpdate and after PreLateUpdate; require both phases.</summary>
        /// <remarks>Existing engine/mod delegates are preserved. No native CPU callback means no markers.</remarks>
        internal static void Install()
        {
            if (Native.ReportCpu == null) return;
            var loop = PlayerLoop.GetCurrentPlayerLoop();
            var nodes = new List<PlayerLoopSystem>();
            bool input = false, simulation = false;
            foreach (var node in loop.subSystemList) {
                if (node.type == typeof(BeforeInput) || node.type == typeof(AfterSimulation)) continue;
                if (node.type == typeof(EarlyUpdate)) {
                    nodes.Add(new PlayerLoopSystem { type = typeof(BeforeInput), updateDelegate = Begin });
                    input = true;
                }
                nodes.Add(node);
                if (node.type == typeof(PreLateUpdate)) {
                    nodes.Add(new PlayerLoopSystem { type = typeof(AfterSimulation), updateDelegate = End });
                    simulation = true;
                }
            }
            if (!input || !simulation) throw new NotSupportedException("Unity input/simulation player-loop phases unavailable.");
            loop.subSystemList = nodes.ToArray(); PlayerLoop.SetPlayerLoop(loop);
        }
        /// <summary>Reserve Time.frameCount+1 and report pacing, input sample and simulation begin.</summary>
        private static void Begin()
        {
            using (var producer = Bootstrap.EnterProducer()) {
                if (!producer.Valid) return;
                ulong frame = (ulong)(uint)Time.frameCount + 1;
                Native.ReportCpu(0, frame); // reserve identity
                Native.ReportCpu(6, frame); // vendor sleep before input
                Native.ReportCpu(1, frame); // input/simulation interval starts
                Native.ReportCpu(2, frame);
            }
        }
        /// <summary>Report simulation end using the same source ID later placed in render packets.</summary>
        private static void End()
        {
            using (var producer = Bootstrap.EnterProducer()) {
                if (producer.Valid) Native.ReportCpu(3, (ulong)(uint)Time.frameCount + 1);
            }
        }
        /// <summary>Remove only this adapter's marker types from the currently installed player loop.</summary>
        internal static void Clear()
        {
            var loop = PlayerLoop.GetCurrentPlayerLoop();
            var nodes = new List<PlayerLoopSystem>();
            foreach (var node in loop.subSystemList)
                if (node.type != typeof(BeforeInput) && node.type != typeof(AfterSimulation)) nodes.Add(node);
            loop.subSystemList = nodes.ToArray(); PlayerLoop.SetPlayerLoop(loop);
        }
    }
}

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
    internal static class CpuBoundaries
    {
        private static IDisposable subscription;
        private static bool legacy = true, mouseKnown;
        private static readonly KeyCode[] keys = (KeyCode[])Enum.GetValues(typeof(KeyCode));
        private static Vector3 mouse;
        private static void ReceivedKinds(uint kinds) {
            using (var producer = Bootstrap.EnterProducer()) if (producer.Valid) Native.InputFact(0, kinds);
        }
        private static void Received()
        {
            using (var producer = Bootstrap.EnterProducer()) {
                if (!producer.Valid) return;
                if (subscription == null) subscription = ReScaleFrame.Engine.InputSystemFacts.AttachLoaded(ReceivedKinds);
                if (subscription != null || !legacy) return;
                try {
                uint kinds = 0;
                foreach (var key in keys) {
                    int code = (int)key;
                    if (code == 0 || (!Input.GetKeyDown(key) && !Input.GetKeyUp(key))) continue;
                    kinds |= code >= (int)KeyCode.JoystickButton0 ? 4u : code >= (int)KeyCode.Mouse0 ? 2u : 1u;
                }
                var current = Input.mousePosition;
                if ((mouseKnown && current != mouse) || Input.mouseScrollDelta != Vector2.zero) kinds |= 2u;
                mouse = current; mouseKnown = true;
                Native.InputFact((ulong)(uint)Time.frameCount + 1, kinds);
                } catch (InvalidOperationException) { legacy = false; }
            }
        }
        internal static void Install()
        {
            if (Native.ReportCpu == null) return;
            subscription = ReScaleFrame.Engine.InputSystemFacts.AttachLoaded(ReceivedKinds);
            ReScaleFrame.Engine.InputLoop.Install(Begin, Received, End);
        }
        private static void Begin()
        {
            using (var producer = Bootstrap.EnterProducer()) {
                if (!producer.Valid) return;
                ulong frame = (ulong)(uint)Time.frameCount + 1;
                Native.ReportCpu(0, frame); // reserve identity
                if (ReScaleFrame.Engine.InputLoop.PollBoundaryAvailable) Native.ReportCpu(1, frame);
                Native.ReportCpu(2, frame);
            }
        }
        private static void End()
        {
            using (var producer = Bootstrap.EnterProducer()) {
                if (producer.Valid) {
                    Native.ReportCpu(3, (ulong)(uint)Time.frameCount + 1);
                    Native.ReportCpu(4, (ulong)(uint)Time.frameCount + 1);
                }
            }
        }
        internal static void Clear()
        {
            subscription?.Dispose(); subscription = null;
            ReScaleFrame.Engine.InputLoop.Clear();
        }
    }
}

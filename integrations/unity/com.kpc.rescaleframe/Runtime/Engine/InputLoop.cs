// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using UnityEngine.LowLevel;
using UnityEngine.PlayerLoop;

namespace ReScaleFrame.Engine
{
    // Engine boundaries only, each reported where it happens. Pacing and missing-event policy
    // belong to the native core. The simulation ends after PreLateUpdate; the main-thread frame,
    // which records and queues the frame's rendering in PostLateUpdate, ends after it. Unity's
    // render thread submits and presents that frame later; the core observes both itself.
    internal static class InputLoop
    {
        private struct BeforeInput { }
        private struct AfterInput { }
        private struct AfterSimulation { }
        private struct AfterFrame { }
        internal static bool PollBoundaryAvailable { get; private set; }
        private static bool Ours(Type type) => type == typeof(BeforeInput) || type == typeof(AfterInput) || type == typeof(AfterSimulation) ||
            type == typeof(AfterFrame);
        internal static PlayerLoopSystem Remove(PlayerLoopSystem loop)
        {
            if (loop.subSystemList == null) return loop;
            var nodes = new List<PlayerLoopSystem>();
            foreach (var node in loop.subSystemList) if (!Ours(node.type)) nodes.Add(Remove(node));
            loop.subSystemList = nodes.ToArray(); return loop;
        }
        internal static PlayerLoopSystem Plan(PlayerLoopSystem loop, PlayerLoopSystem.UpdateFunction begin,
            PlayerLoopSystem.UpdateFunction received, PlayerLoopSystem.UpdateFunction simulated,
            PlayerLoopSystem.UpdateFunction finished)
        {
            loop = Remove(loop); PollBoundaryAvailable = false;
            var nodes = new List<PlayerLoopSystem>();
            foreach (var original in loop.subSystemList ?? Array.Empty<PlayerLoopSystem>()) {
                var node = original;
                if (node.type == typeof(EarlyUpdate)) {
                    var children = node.subSystemList ?? Array.Empty<PlayerLoopSystem>();
                    int first = -1, last = -1;
                    for (int i = 0; i < children.Length; ++i) {
                        string name = children[i].type?.Name;
                        if (name != "UpdateInputManager" && name != "NewInputUpdate") continue;
                        if (first < 0) first = i; last = i;
                    }
                    if (first >= 0) {
                        PollBoundaryAvailable = true;
                        var input = new List<PlayerLoopSystem>();
                        for (int i = 0; i < children.Length; ++i) {
                            if (i == first) input.Add(new PlayerLoopSystem { type = typeof(BeforeInput), updateDelegate = begin });
                            input.Add(children[i]);
                            if (i == last) input.Add(new PlayerLoopSystem { type = typeof(AfterInput), updateDelegate = received });
                        }
                        node.subSystemList = input.ToArray();
                    } else {
                        // Reserve an engine update only. The caller reports no measured input poll.
                        nodes.Add(new PlayerLoopSystem { type = typeof(BeforeInput), updateDelegate = begin });
                    }
                }
                nodes.Add(node);
                if (node.type == typeof(PreLateUpdate)) nodes.Add(new PlayerLoopSystem { type = typeof(AfterSimulation), updateDelegate = simulated });
                if (node.type == typeof(PostLateUpdate)) nodes.Add(new PlayerLoopSystem { type = typeof(AfterFrame), updateDelegate = finished });
            }
            loop.subSystemList = nodes.ToArray(); return loop;
        }
        internal static void Install(PlayerLoopSystem.UpdateFunction begin, PlayerLoopSystem.UpdateFunction received,
            PlayerLoopSystem.UpdateFunction simulated, PlayerLoopSystem.UpdateFunction finished)
            => PlayerLoop.SetPlayerLoop(Plan(PlayerLoop.GetCurrentPlayerLoop(), begin, received, simulated, finished));
        internal static void Clear() => PlayerLoop.SetPlayerLoop(Remove(PlayerLoop.GetCurrentPlayerLoop()));
    }
}

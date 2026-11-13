// SPDX-License-Identifier: GPL-3.0-only
using System;
using System.Collections.Generic;
using UnityEngine.LowLevel;
using UnityEngine.PlayerLoop;

namespace ReScaleFrame.Engine
{
    // Engine boundaries only. Pacing and missing-event policy belong to the native core.
    internal static class InputLoop
    {
        private struct BeforeInput { }
        private struct AfterInput { }
        private struct AfterSimulation { }
        internal static bool PollBoundaryAvailable { get; private set; }
        private static bool Ours(Type type) => type == typeof(BeforeInput) || type == typeof(AfterInput) || type == typeof(AfterSimulation);
        internal static PlayerLoopSystem Remove(PlayerLoopSystem loop)
        {
            if (loop.subSystemList == null) return loop;
            var nodes = new List<PlayerLoopSystem>();
            foreach (var node in loop.subSystemList) if (!Ours(node.type)) nodes.Add(Remove(node));
            loop.subSystemList = nodes.ToArray(); return loop;
        }
        internal static PlayerLoopSystem Plan(PlayerLoopSystem loop, PlayerLoopSystem.UpdateFunction begin,
            PlayerLoopSystem.UpdateFunction received, PlayerLoopSystem.UpdateFunction end)
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
                if (node.type == typeof(PreLateUpdate)) nodes.Add(new PlayerLoopSystem { type = typeof(AfterSimulation), updateDelegate = end });
            }
            loop.subSystemList = nodes.ToArray(); return loop;
        }
        internal static void Install(PlayerLoopSystem.UpdateFunction begin, PlayerLoopSystem.UpdateFunction received, PlayerLoopSystem.UpdateFunction end)
            => PlayerLoop.SetPlayerLoop(Plan(PlayerLoop.GetCurrentPlayerLoop(), begin, received, end));
        internal static void Clear() => PlayerLoop.SetPlayerLoop(Remove(PlayerLoop.GetCurrentPlayerLoop()));
    }
}

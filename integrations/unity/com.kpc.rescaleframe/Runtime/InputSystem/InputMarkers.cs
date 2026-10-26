// SPDX-License-Identifier: GPL-3.0-only
using UnityEngine;
using UnityEngine.InputSystem;
using UnityEngine.InputSystem.LowLevel;

namespace ReScaleFrame
{
    internal static class InputMarkers
    {
        [RuntimeInitializeOnLoadMethod(RuntimeInitializeLoadType.BeforeSceneLoad)]
        private static void Install() { InputSystem.onEvent -= OnEvent; InputSystem.onEvent += OnEvent; }
        private static void OnEvent(InputEventPtr input, InputDevice device) {
            if (!input.IsA<StateEvent>() && !input.IsA<DeltaStateEvent>()) return;
            uint kind = device is Gamepad ? 4u : device is Mouse ? 2u : device is Keyboard ? 1u : 0u;
            if (kind != 0) ReScaleFrameRuntime.NotifyInput(kind);
        }
    }
}

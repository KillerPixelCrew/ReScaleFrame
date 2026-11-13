// SPDX-License-Identifier: GPL-3.0-only
using System;
using System.Reflection;

namespace ReScaleFrame.Engine
{
    // Optional Input System receipt observation without a mandatory package assembly dependency.
    internal static class InputSystemFacts
    {
        internal static IDisposable Attach(Type inputSystem, Action<uint> received)
        {
            try { return AttachCore(inputSystem, received); }
            catch (Exception error) when (error is System.Reflection.TargetInvocationException || error is ArgumentException || error is InvalidOperationException || error is MemberAccessException) { return null; }
        }
        private static IDisposable AttachCore(Type inputSystem, Action<uint> received)
        {
            var property = inputSystem?.GetProperty("onEvent", BindingFlags.Public | BindingFlags.Static);
            if (property == null) return null;
            object listener = property.GetValue(null);
            if (listener == null) return null;
            foreach (var contract in listener.GetType().GetInterfaces()) {
                if (!contract.IsGenericType || contract.GetGenericTypeDefinition() != typeof(IObservable<>)) continue;
                var method = typeof(InputSystemFacts).GetMethod(nameof(Subscribe), BindingFlags.NonPublic | BindingFlags.Static);
                return (IDisposable)method.MakeGenericMethod(contract.GetGenericArguments()[0]).Invoke(null, new object[] { listener, inputSystem, received });
            }
            return null;
        }
        internal static IDisposable AttachLoaded(Action<uint> received)
        {
            foreach (var assembly in AppDomain.CurrentDomain.GetAssemblies()) {
                if (assembly.GetName().Name != "Unity.InputSystem") continue;
                return Attach(assembly.GetType("UnityEngine.InputSystem.InputSystem"), received);
            }
            return null;
        }
        private static IDisposable Subscribe<T>(object listener, Type system, Action<uint> received)
            => ((IObservable<T>)listener).Subscribe(new Receiver<T>(system, received));
        private sealed class Receiver<T> : IObserver<T>
        {
            private readonly PropertyInfo eventType = typeof(T).GetProperty("type"), deviceId = typeof(T).GetProperty("deviceId");
            private readonly MethodInfo device;
            private readonly Action<uint> received;
            internal Receiver(Type system, Action<uint> callback) { device = system.GetMethod("GetDeviceById"); received = callback; }
            public void OnNext(T value)
            {
                string kind = eventType?.GetValue(value)?.ToString();
                if ((kind != "STAT" && kind != "DLTA") || device == null || deviceId == null) return;
                var input = device.Invoke(null, new[] { deviceId.GetValue(value) });
                for (var type = input?.GetType(); type != null; type = type.BaseType) {
                    uint mask = type.Name == "Keyboard" ? 1u : type.Name == "Mouse" ? 2u :
                        type.Name == "Gamepad" || type.Name == "Joystick" ? 4u : 0u;
                    if (mask == 0) continue;
                    received(mask); return;
                }
            }
            public void OnError(Exception error) { }
            public void OnCompleted() { }
        }
    }
}

// SPDX-License-Identifier: GPL-3.0-only
#if ENABLE_UPSCALER_FRAMEWORK
using System;
using System.Reflection;
using UnityEditor;
using UnityEngine;

namespace ReScaleFrame.Editor
{
    [InitializeOnLoad]
    internal static class GameViewPreview
    {
        private const BindingFlags Flags = BindingFlags.Instance | BindingFlags.Public | BindingFlags.NonPublic;
        private static readonly Type ViewType = typeof(EditorWindow).Assembly.GetType("UnityEditor.GameView");
        private static readonly FieldInfo Texture = ViewType?.GetField("m_RenderTexture", Flags);
        private static readonly PropertyInfo ViewRect = ViewType?.GetProperty("viewInWindow", Flags);
        private static readonly PropertyInfo TargetRect = ViewType?.GetProperty("targetInView", Flags);
        private static readonly FieldInfo Parent = typeof(EditorWindow).GetField("m_Parent", Flags);
        static GameViewPreview() {
            EditorApplication.update += Update;
            EditorApplication.playModeStateChanged += _ => Update();
        }
        private static void Update() {
            if (!EditorApplication.isPlaying || Texture == null || ViewRect == null || TargetRect == null) {
                ReScaleFrameRuntime.ConfigureEditorPreview(null, default, false); return;
            }
            foreach (var window in Resources.FindObjectsOfTypeAll<EditorWindow>()) {
                if (window.GetType() != ViewType) continue;
                object parent = Parent?.GetValue(window);
                var actualView = parent?.GetType().GetProperty("actualView", Flags)?.GetValue(parent) as EditorWindow;
                if (actualView != null && actualView != window) continue;
                var view = (Rect)ViewRect.GetValue(window);
                var target = (Rect)TargetRect.GetValue(window);
                var position = window.position.position + view.position + target.position;
                float dpi = EditorGUIUtility.pixelsPerPoint;
                var pixels = new Rect(position * dpi, target.size * dpi);
                ReScaleFrameRuntime.ConfigureEditorPreview(Texture.GetValue(window) as RenderTexture, pixels, true); return;
            }
            ReScaleFrameRuntime.ConfigureEditorPreview(null, default, false);
        }
    }
}
#endif

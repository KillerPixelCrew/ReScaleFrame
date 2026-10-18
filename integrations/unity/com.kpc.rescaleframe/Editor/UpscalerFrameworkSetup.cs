// SPDX-License-Identifier: GPL-3.0-only
using UnityEditor;
using UnityEditor.Build;

namespace ReScaleFrame.Editor
{
    internal static class UpscalerFrameworkSetup
    {
        private const string Symbol = "ENABLE_UPSCALER_FRAMEWORK";
        [InitializeOnLoadMethod]
        private static void Initialize() {
            EditorApplication.delayCall += Enable;
        }
        [MenuItem("Tools/ReScaleFrame/Enable URP Upscaler Framework")]
        private static void Enable() {
            var target = NamedBuildTarget.Standalone;
            string current = PlayerSettings.GetScriptingDefineSymbols(target);
            foreach (string item in current.Split(';')) if (item == Symbol) return;
            PlayerSettings.SetScriptingDefineSymbols(target, string.IsNullOrEmpty(current) ? Symbol : current + ";" + Symbol);
        }
    }
}

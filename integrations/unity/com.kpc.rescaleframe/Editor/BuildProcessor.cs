// SPDX-License-Identifier: GPL-3.0-only
#if ENABLE_UPSCALER_FRAMEWORK
using System;
using System.IO;
using UnityEditor;
using UnityEditor.Build;
using UnityEditor.Build.Reporting;
using UnityEngine;

namespace ReScaleFrame.Editor
{
    internal sealed class BuildProcessor : IPreprocessBuildWithReport, IPostprocessBuildWithReport
    {
        public int callbackOrder => 0;
        private static string PackagePath => Path.GetFullPath(UnityEditor.PackageManager.PackageInfo.FindForAssembly(typeof(BuildProcessor).Assembly).resolvedPath);
        private static string IdentityPath => Path.Combine(Directory.GetCurrentDirectory(), "ProjectSettings", "ReScaleFrame.json");
        [Serializable] private sealed class Identity { public string projectId; }
        internal static string ProjectId {
            get {
                if (!string.IsNullOrEmpty(Application.cloudProjectId)) return Application.cloudProjectId;
                if (File.Exists(IdentityPath)) return JsonUtility.FromJson<Identity>(File.ReadAllText(IdentityPath)).projectId;
                var identity = new Identity { projectId = Guid.NewGuid().ToString() };
                File.WriteAllText(IdentityPath, JsonUtility.ToJson(identity, true)); return identity.projectId;
            }
        }
        public void OnPreprocessBuild(BuildReport report) {
            if (report.summary.platform != BuildTarget.StandaloneWindows64) throw new BuildFailedException("ReScaleFrame currently targets Windows x64 players.");
            foreach (var process in System.Diagnostics.Process.GetProcessesByName(Path.GetFileNameWithoutExtension(report.summary.outputPath))) {
                try {
                    if (string.Equals(process.MainModule.FileName, Path.GetFullPath(report.summary.outputPath), StringComparison.OrdinalIgnoreCase))
                        throw new BuildFailedException("Close the running player before rebuilding its files.");
                } finally { process.Dispose(); }
            }
            string native = Path.Combine(PackagePath, "Plugins", "x86_64", "ReScaleFrame.Unity.Native.dll");
            if (!File.Exists(native) || !Directory.Exists(Path.Combine(PackagePath, "Runtime~")))
                throw new BuildFailedException("Build and stage the ReScaleFrame Unity package before building this player.");
            var importer = AssetImporter.GetAtPath("Packages/com.kpc.rescaleframe/Plugins/x86_64/ReScaleFrame.Unity.Native.dll") as PluginImporter;
            if (importer == null || !importer.isPreloaded) throw new BuildFailedException("ReScaleFrame.Unity.Native.dll must be enabled for Windows x64 and Load on startup must be enabled.");
            _ = ProjectId;
        }
        public void OnPostprocessBuild(BuildReport report) {
            string data = Path.Combine(Path.GetDirectoryName(report.summary.outputPath), Path.GetFileNameWithoutExtension(report.summary.outputPath) + "_Data");
            string plugins = Path.Combine(data, "Plugins", "x86_64"); Directory.CreateDirectory(plugins);
            string target = Path.Combine(data, "StreamingAssets", "ReScaleFrame");
            CopyTree(Path.Combine(PackagePath, "Runtime~"), target);
            string runtime = Path.GetRelativePath(plugins, target).Replace('\\', '/');
            File.WriteAllLines(Path.Combine(plugins, "ReScaleFrame.Unity.ini"), new[] {
                "[UnitySR]", "Runtime=" + runtime, "Log=" + Path.Combine(data, "ReScaleFrame.log"),
                "EngineVersion=" + Application.unityVersion, "ProjectId=" + ProjectId
            }, System.Text.Encoding.Unicode);
        }
        private static void CopyTree(string source, string target) {
            Directory.CreateDirectory(target);
            foreach (string file in Directory.GetFiles(source)) File.Copy(file, Path.Combine(target, Path.GetFileName(file)), true);
            foreach (string directory in Directory.GetDirectories(source)) CopyTree(directory, Path.Combine(target, Path.GetFileName(directory)));
        }
    }
}
#endif

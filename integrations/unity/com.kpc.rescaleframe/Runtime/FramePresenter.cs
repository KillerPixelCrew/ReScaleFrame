// SPDX-License-Identifier: GPL-3.0-only
#if ENABLE_UPSCALER_FRAMEWORK
using System.Collections;
using UnityEngine;

namespace ReScaleFrame
{
    internal sealed class FramePresenter : MonoBehaviour
    {
        private static FramePresenter instance;
        [RuntimeInitializeOnLoadMethod(RuntimeInitializeLoadType.BeforeSceneLoad)]
        private static void Install() {
            if (instance != null) return;
            var owner = new GameObject("ReScaleFrame frame presentation") { hideFlags = HideFlags.HideInHierarchy };
            DontDestroyOnLoad(owner); instance = owner.AddComponent<FramePresenter>();
        }
        private void OnDestroy() { if (instance == this) instance = null; }
        private void OnEnable() { StartCoroutine(Present()); }
        private IEnumerator Present() {
            var completed = new WaitForEndOfFrame();
            while (true) { yield return completed; ReScaleFrameRuntime.PresentCompletedFrame(); }
        }
    }
}
#endif

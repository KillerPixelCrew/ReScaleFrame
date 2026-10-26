// SPDX-License-Identifier: GPL-3.0-only
#if ENABLE_UPSCALER_FRAMEWORK
namespace ReScaleFrame
{
    public readonly struct ReScaleFrameFrameRates
    {
        public double BaseFPS { get; }
        public double OutputFPS { get; }
        public bool HasOutputFPS { get; }
        internal ReScaleFrameFrameRates(double baseFPS, double outputFPS, bool hasOutputFPS) {
            BaseFPS = baseFPS; OutputFPS = outputFPS; HasOutputFPS = hasOutputFPS;
        }
    }
}
#endif

// SPDX-License-Identifier: MIT
// Extract the Streamline and FSR 2 facts using the actual vendor declarations. Both SDKs declare
// C++ types (namespaces, constructors and virtual members), so they cannot be measured from C.
// The other SDKs are measured in vendor_layout_facts.c.

#include <windows.h>

#include <cstddef>
#include <cstdint>

#if defined(RSF_CENSUS_LAYOUT_SL)
#include <sl.h>
#include <sl_consts.h>
#include <sl_dlss.h>
#include <sl_dlss_g.h>
#include <sl_pcl.h>
#include <sl_reflex.h>
#endif
#if defined(RSF_CENSUS_LAYOUT_FSR2)
#include <ffx_fsr2.h>
#endif

using Visit = void (*)(void*, uint32_t, const size_t*, size_t);

namespace {

// Offsets of members of types with constructors or virtual functions: measure on real storage.
template <class T, class M>
size_t member_offset(M T::*member) {
    alignas(64) static unsigned char storage[8192];
    T* object = reinterpret_cast<T*>(storage);
    return static_cast<size_t>(reinterpret_cast<unsigned char*>(&(object->*member)) - storage);
}

#define OFFSET(Type, member) member_offset(&Type::member)

} // namespace

extern "C" void rsf_test_layout_facts_cxx(void* context, Visit visit) {
    (void)context;
    (void)visit;
#if defined(RSF_CENSUS_LAYOUT_SL)
    {
        const size_t values[] = {
            sizeof(sl::BaseStructure),
            OFFSET(sl::Preferences, showConsole),
            OFFSET(sl::Preferences, logLevel),
            OFFSET(sl::Preferences, numPathsToPlugins),
            OFFSET(sl::Preferences, pathToLogsAndData),
            OFFSET(sl::Preferences, flags),
            OFFSET(sl::Preferences, featuresToLoad),
            OFFSET(sl::Preferences, numFeaturesToLoad),
            OFFSET(sl::Preferences, applicationId),
            OFFSET(sl::Preferences, engine),
            OFFSET(sl::Preferences, engineVersion),
            OFFSET(sl::Preferences, projectId),
            OFFSET(sl::Preferences, renderAPI),
            OFFSET(sl::ResourceTag, resource),
            OFFSET(sl::ResourceTag, type),
            OFFSET(sl::ResourceTag, lifecycle),
            OFFSET(sl::ResourceTag, extent),
            sizeof(sl::ResourceTag),
            OFFSET(sl::Resource, type),
            OFFSET(sl::Resource, native),
            OFFSET(sl::Resource, state),
            OFFSET(sl::Resource, width),
            OFFSET(sl::Resource, height),
            OFFSET(sl::Resource, nativeFormat),
            OFFSET(sl::Constants, jitterOffset),
            OFFSET(sl::Constants, cameraViewToClip),
            OFFSET(sl::Constants, clipToCameraView),
            OFFSET(sl::Constants, clipToLensClip),
            OFFSET(sl::Constants, clipToPrevClip),
            OFFSET(sl::Constants, prevClipToClip),
            OFFSET(sl::Constants, cameraPos),
            OFFSET(sl::Constants, cameraUp),
            OFFSET(sl::Constants, cameraRight),
            OFFSET(sl::Constants, cameraFwd),
            OFFSET(sl::Constants, mvecScale),
            OFFSET(sl::Constants, cameraNear),
            OFFSET(sl::Constants, cameraFar),
            OFFSET(sl::Constants, cameraFOV),
            OFFSET(sl::Constants, cameraAspectRatio),
            OFFSET(sl::Constants, depthInverted),
            OFFSET(sl::Constants, cameraMotionIncluded),
            OFFSET(sl::Constants, motionVectors3D),
            OFFSET(sl::Constants, reset),
            OFFSET(sl::Constants, orthographicProjection),
            OFFSET(sl::Constants, motionVectorsDilated),
            OFFSET(sl::Constants, motionVectorsJittered),
            OFFSET(sl::DLSSGOptions, mode),
            OFFSET(sl::DLSSGOptions, numFramesToGenerate),
            OFFSET(sl::DLSSGOptions, flags),
            OFFSET(sl::DLSSGOptions, dynamicResWidth),
            OFFSET(sl::DLSSGOptions, numBackBuffers),
            OFFSET(sl::DLSSGOptions, mvecDepthWidth),
            OFFSET(sl::DLSSGOptions, colorWidth),
            OFFSET(sl::DLSSGOptions, colorBufferFormat),
            OFFSET(sl::DLSSGOptions, uiBufferFormat),
            OFFSET(sl::DLSSOptions, mode),
            OFFSET(sl::DLSSOptions, outputWidth),
            OFFSET(sl::DLSSOptions, outputHeight),
            OFFSET(sl::DLSSOptions, sharpness),
            OFFSET(sl::DLSSOptions, preExposure),
            OFFSET(sl::DLSSOptions, exposureScale),
            OFFSET(sl::ReflexOptions, mode),
            OFFSET(sl::ReflexOptions, frameLimitUs),
            OFFSET(sl::ReflexOptions, useMarkersToOptimize),
            OFFSET(sl::ReflexOptions, virtualKey),
            OFFSET(sl::ReflexOptions, idThread),
            sizeof(sl::ViewportHandle),
        };
        visit(context, 0, values, sizeof(values) / sizeof(values[0]));
    }
#endif
#if defined(RSF_CENSUS_LAYOUT_FSR2)
    {
        const size_t values[] = {
            sizeof(Fsr212::FfxResource),
            OFFSET(Fsr212::FfxFsr2DispatchDescription, color),
            OFFSET(Fsr212::FfxFsr2DispatchDescription, output),
            OFFSET(Fsr212::FfxFsr2DispatchDescription, jitterOffset),
            OFFSET(Fsr212::FfxFsr2DispatchDescription, renderSize),
            OFFSET(Fsr212::FfxFsr2DispatchDescription, enableSharpening),
            OFFSET(Fsr212::FfxFsr2DispatchDescription, reset),
            OFFSET(Fsr212::FfxFsr2DispatchDescription, cameraFovAngleVertical),
            sizeof(Fsr212::FfxFsr2DispatchDescription),
            OFFSET(Fsr212::FfxResource, name),
            OFFSET(Fsr212::FfxResource, description),
            OFFSET(Fsr212::FfxResource, state),
            OFFSET(Fsr212::FfxResource, isDepth),
        };
        visit(context, 4, values, sizeof(values) / sizeof(values[0]));
    }
#endif
}

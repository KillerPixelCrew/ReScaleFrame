// SPDX-License-Identifier: GPL-3.0-only
#include <jni.h>
#include <rescaleframe/gl_pipeline.h>
#include <rescaleframe/input_dump.h>
#include <rescaleframe/native_cpu.h>
#include <windows.h>
#include <string>
#include <cstdio>
#include <cstring>
#include <array>
#include <algorithm>
#include "input_observer.h"
namespace {
using Pipeline=rsf_gl_pipeline;
std::atomic<Pipeline*> input_pipeline{};
FILE* logfile=nullptr;
void log(void*,const char* message) {if(logfile) {std::fprintf(logfile,"%s\n",message);std::fflush(logfile);}}
struct Text {
    JNIEnv* env;jstring value;const char* text;
    Text(JNIEnv* e,jstring v):env(e),value(v),text(v?e->GetStringUTFChars(v,nullptr):nullptr) {}
    ~Text() {if(text)env->ReleaseStringUTFChars(value,text);}
};
Pipeline* pipeline(jlong pointer) {return reinterpret_cast<Pipeline*>(static_cast<uintptr_t>(pointer));}
rsf_game_render_pass pass(JNIEnv* env,Pipeline* self,jlong frame,jobject camera,jboolean reset,jint screen) {
    rsf_game_render_pass out{};out.struct_size=sizeof(out);
    auto* data=static_cast<float*>(env->GetDirectBufferAddress(camera));
    if(!data||env->GetDirectBufferCapacity(camera)<88)return out;
    rsf_gl_pipeline_surfaces surfaces{};surfaces.struct_size=sizeof(surfaces);rsf_gl_pipeline_get_surfaces(self,&surfaces);
    auto& c=out.camera;c.struct_size=sizeof(c);c.abi_version=RSF_GAME_FRAME_ABI_VERSION;
    std::memcpy(c.view_to_clip,data,80*sizeof(float));
    c.jitter_pixels[0]=data[80];c.jitter_pixels[1]=data[81];c.previous_jitter_pixels[0]=data[82];c.previous_jitter_pixels[1]=data[83];
    c.near_plane=data[84];c.far_plane=data[85];c.vertical_fov_radians=data[86];c.frame_time_seconds=data[87];
    c.render_width=surfaces.render_width;c.render_height=surfaces.render_height;c.output_width=surfaces.output_width;c.output_height=surfaces.output_height;
    out.role=RSF_GAME_RENDER_SR;out.native_frame=out.source_frame_id=static_cast<uint64_t>(frame);out.camera_valid=1;out.view_key=out.history_key=1;
    out.flags=RSF_GAME_RENDER_PRIMARY|(reset?RSF_GAME_RENDER_RESET:0);out.screen=static_cast<uint32_t>(screen);
    out.render_rect[2]=static_cast<int32_t>(c.render_width);out.render_rect[3]=static_cast<int32_t>(c.render_height);
    out.output_rect[2]=static_cast<int32_t>(c.output_width);out.output_rect[3]=static_cast<int32_t>(c.output_height);
    out.motion_to_uv[0]=1.0f/c.render_width;out.motion_to_uv[1]=1.0f/c.render_height;out.motion_camera_included=1;
    return out;
}
}
#define JNI_METHOD(name) extern "C" JNIEXPORT name
JNI_METHOD(jlong) JNICALL Java_org_kpc_rescaleframecraft_render_NativeBridge_createSurface(JNIEnv*,jclass,jlong parent) {
    auto owner=reinterpret_cast<HWND>(static_cast<uintptr_t>(parent));
    auto surface=static_cast<HWND>(rsf_gl_window_create(owner));
    if(surface&&!MinecraftInputObserver::instance().install(owner,surface,[](UINT message) {
        (void)rsf_gl_pipeline_input(input_pipeline.load(),0,0,message);
    })) {rsf_gl_window_destroy(surface);return 0;}
    return static_cast<jlong>(reinterpret_cast<uintptr_t>(surface));
}
JNI_METHOD(void) JNICALL Java_org_kpc_rescaleframecraft_render_NativeBridge_destroySurface(JNIEnv*,jclass,jlong window) {
    MinecraftInputObserver::instance().close();
    rsf_gl_window_destroy(reinterpret_cast<void*>(static_cast<uintptr_t>(window)));
}
JNI_METHOD(jlong) JNICALL Java_org_kpc_rescaleframecraft_render_NativeBridge_open(JNIEnv* env,jclass,jlong hwnd,jlong surface,jstring root) {
    Text path(env,root);if(!path.text)return 0;
    if(!logfile) {const std::string file=std::string(path.text)+"/minecraft-native.log";logfile=std::fopen(file.c_str(),"a");}
    rsf_gl_pipeline_setup setup{sizeof(setup),reinterpret_cast<void*>(static_cast<uintptr_t>(hwnd)),path.text,log,nullptr,"Minecraft 1.21.1","11e3ed7f-06c6-4863-a54d-2b790f2176f2",reinterpret_cast<void*>(static_cast<uintptr_t>(surface))};
    rsf_input_dump_configure(nullptr,"Minecraft",nullptr,log,nullptr);
    Pipeline* self=nullptr;const auto result=rsf_gl_pipeline_open(&setup,&self);
    if(result!=RSF_BACKEND_OK) {char message[96];std::snprintf(message,sizeof(message),"Minecraft native open failed: %d",result);log(nullptr,message);}
    input_pipeline.store(self);
    return static_cast<jlong>(reinterpret_cast<uintptr_t>(self));
}
JNI_METHOD(jintArray) JNICALL Java_org_kpc_rescaleframecraft_render_NativeBridge_configure(JNIEnv* env,jclass,jlong pointer,jint width,jint height,jint sr,jint quality,jint preset,jint fg,jint count,jint reflex,jint limit,jboolean debug,jfloatArray sharpening) {
    rsf_gl_pipeline_options options{};options.struct_size=sizeof(options);options.sr_backend=sr;options.quality=quality;options.dlss_preset=preset;options.fg_backend=fg;
    options.generation={sizeof(options.generation),fg?RSF_FG_FIXED:RSF_FG_OFF,static_cast<uint32_t>(count),static_cast<uint32_t>(reflex),limit>0?1000000u/static_cast<uint32_t>(limit):0u,debug?1u:0u};
    if(!sharpening||env->GetArrayLength(sharpening)!=12)return nullptr;
    jfloat sharpening_values[12]{};env->GetFloatArrayRegion(sharpening,0,12,sharpening_values);if(env->ExceptionCheck())return nullptr;
    options.sharpening=rsf_sharpening_defaults();
    options.sharpening.mode=(std::isfinite(sharpening_values[0])&&sharpening_values[0]>=0&&sharpening_values[0]<=4?static_cast<uint32_t>(sharpening_values[0]):UINT32_MAX);
    options.sharpening.sharpness=sharpening_values[1];
    options.sharpening.mas=(std::isfinite(sharpening_values[2])&&sharpening_values[2]>=0&&sharpening_values[2]<=4?static_cast<uint32_t>(sharpening_values[2]):UINT32_MAX);
    options.sharpening.motion_sharpness=sharpening_values[3];
    options.sharpening.motion_threshold=sharpening_values[4];
    options.sharpening.motion_scale_limit=sharpening_values[5];
    options.sharpening.contrast_enabled=(std::isfinite(sharpening_values[6])&&sharpening_values[6]>=0&&sharpening_values[6]<=4?static_cast<uint32_t>(sharpening_values[6]):UINT32_MAX);
    options.sharpening.contrast=sharpening_values[7];
    options.sharpening.depth_scale=sharpening_values[8];
    options.sharpening.depth_bias=sharpening_values[9];
    options.sharpening.clamp_output=(std::isfinite(sharpening_values[10])&&sharpening_values[10]>=0&&sharpening_values[10]<=4?static_cast<uint32_t>(sharpening_values[10]):UINT32_MAX);
    options.sharpening.debug=(std::isfinite(sharpening_values[11])&&sharpening_values[11]>=0&&sharpening_values[11]<=4?static_cast<uint32_t>(sharpening_values[11]):UINT32_MAX);
    if(!rsf::sharpening_settings::valid(options.sharpening))return nullptr;
    const auto result=rsf_gl_pipeline_configure(pipeline(pointer),&options,width,height);
    rsf_gl_pipeline_surfaces surfaces{};surfaces.struct_size=sizeof(surfaces);
    if(result!=RSF_BACKEND_OK||rsf_gl_pipeline_get_surfaces(pipeline(pointer),&surfaces)!=RSF_BACKEND_OK)return nullptr;
    const std::array<jint,10> values{static_cast<jint>(surfaces.render_width),static_cast<jint>(surfaces.render_height),width,height,
        static_cast<jint>(surfaces.scene),static_cast<jint>(surfaces.depth),static_cast<jint>(surfaces.motion),static_cast<jint>(surfaces.output),
        static_cast<jint>(surfaces.hudless),static_cast<jint>(surfaces.composed)};
    auto array=env->NewIntArray(static_cast<jsize>(values.size()));if(array)env->SetIntArrayRegion(array,0,static_cast<jsize>(values.size()),values.data());return array;
}
JNI_METHOD(void) JNICALL Java_org_kpc_rescaleframecraft_render_NativeBridge_cpu(JNIEnv*,jclass,jlong pointer,jlong frame,jint stage) {
    auto* self=pipeline(pointer);
    rsf_gl_pipeline_cpu(self,frame,stage);
}
JNI_METHOD(void) JNICALL Java_org_kpc_rescaleframecraft_render_NativeBridge_input(JNIEnv*,jclass,jlong pointer,jlong frame,jint kinds) {
    rsf_gl_pipeline_input(pipeline(pointer),frame,static_cast<uint32_t>(kinds),0);
}
JNI_METHOD(void) JNICALL Java_org_kpc_rescaleframecraft_render_NativeBridge_abort(JNIEnv*,jclass,jlong pointer,jlong frame) {rsf_gl_pipeline_abort(pipeline(pointer),frame);}
JNI_METHOD(jint) JNICALL Java_org_kpc_rescaleframecraft_render_NativeBridge_evaluate(JNIEnv* env,jclass,jlong pointer,jlong frame,jobject camera,jboolean reset,jint screen) {
    const auto input=pass(env,pipeline(pointer),frame,camera,reset,screen);return rsf_gl_pipeline_evaluate(pipeline(pointer),&input);
}
JNI_METHOD(jint) JNICALL Java_org_kpc_rescaleframecraft_render_NativeBridge_hudless(JNIEnv* env,jclass,jlong pointer,jlong frame,jobject camera) {
    const auto input=pass(env,pipeline(pointer),frame,camera,false,RSF_SCREEN_FLIGHT);return rsf_gl_pipeline_hudless(pipeline(pointer),&input);
}
JNI_METHOD(jint) JNICALL Java_org_kpc_rescaleframecraft_render_NativeBridge_present(JNIEnv*,jclass,jlong pointer,jlong frame,jboolean vsync) {return rsf_gl_pipeline_present(pipeline(pointer),frame,vsync);}
JNI_METHOD(void) JNICALL Java_org_kpc_rescaleframecraft_render_NativeBridge_status(JNIEnv* env,jclass,jlong pointer,jlongArray destination) {
    if(!destination||env->GetArrayLength(destination)<14)return;
    rsf_gl_pipeline_status status{};status.struct_size=sizeof(status);rsf_gl_pipeline_status_get(pipeline(pointer),&status);
    const auto& fg=status.generation;
    rsf_native_latency_status latency{sizeof(latency),1};rsf_native_latency_status_get(&latency);
    const std::array<jlong,25> values{status.requested_sr,status.effective_sr,status.requested_fg,status.effective_fg,status.sr_result,status.fg_switch_result,
        static_cast<jlong>(status.evaluated),fg.reason,fg.last_result,fg.vendor.effective_generated_frames,static_cast<jlong>(fg.tagged_frames),static_cast<jlong>(fg.vendor.generated_presented),fg.vendor.active,fg.vendor.valid_statistics,
        fg.vendor.max_generated_frames,fg.vendor.supported,latency.owner,static_cast<jlong>(latency.paced_frames),
        static_cast<jlong>(latency.events.input_events[0]),static_cast<jlong>(latency.events.input_events[1]),
        static_cast<jlong>(latency.events.input_events[2]),static_cast<jlong>(latency.events.pings),
        static_cast<jlong>(latency.events.substitutes),static_cast<jlong>(latency.events.order_violations),
        static_cast<jlong>(latency.derived_boundaries)};
    env->SetLongArrayRegion(destination,0,std::min(env->GetArrayLength(destination),static_cast<jsize>(values.size())),values.data());
}
JNI_METHOD(jint) JNICALL Java_org_kpc_rescaleframecraft_render_NativeBridge_close(JNIEnv*,jclass,jlong pointer) {
    input_pipeline.store(nullptr);return rsf_gl_pipeline_close(pipeline(pointer));
}

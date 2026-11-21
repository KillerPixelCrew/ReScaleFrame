// SPDX-License-Identifier: MIT
// Typed JNI leaves for org.kpc.rescaleframecraft.render.NativeBridge. The Rust owner (lib.rs) holds
// the session, log, input observer, camera packet, configuration and status policy; this file
// marshals Java values and lends it the linked Runtime through minecraft_platform.h.
#include <jni.h>
#include <rescaleframe/gl_pipeline.h>
#include <rescaleframe/input_dump.h>
#include <rescaleframe/native_cpu.h>
#include <array>
#include <cstdint>
#include "minecraft_platform.h"
static_assert(RSF_MINECRAFT_HUDLESS_SCREEN==RSF_SCREEN_FLIGHT);
namespace {
const rsf_minecraft_platform* platform() {
    static const rsf_minecraft_platform table{
        rsf_gl_window_create,rsf_gl_window_destroy,rsf_gl_pipeline_open,rsf_gl_pipeline_configure,
        rsf_gl_pipeline_get_surfaces,rsf_gl_pipeline_cpu,rsf_gl_pipeline_input,rsf_gl_pipeline_abort,
        rsf_gl_pipeline_evaluate,rsf_gl_pipeline_hudless,rsf_gl_pipeline_present,rsf_gl_pipeline_status_get,
        rsf_gl_pipeline_close,rsf_input_dump_configure,rsf_native_latency_status_get,
        [](rsf_sharpening_options* options) {*options=rsf_sharpening_defaults();},
        [](const rsf_sharpening_options* options) {return rsf::sharpening_settings::valid(*options);}};
    return &table;
}
struct Text {
    JNIEnv* env;jstring value;const char* text;
    Text(JNIEnv* e,jstring v):env(e),value(v),text(v?e->GetStringUTFChars(v,nullptr):nullptr) {}
    ~Text() {if(text)env->ReleaseStringUTFChars(value,text);}
};
rsf_gl_pipeline* pipeline(jlong pointer) {return reinterpret_cast<rsf_gl_pipeline*>(static_cast<uintptr_t>(pointer));}
void* window(jlong value) {return reinterpret_cast<void*>(static_cast<uintptr_t>(value));}
jlong handle(const void* value) {return static_cast<jlong>(reinterpret_cast<uintptr_t>(value));}
// jint is long on Windows; the platform header carries int32_t.
static_assert(sizeof(jint)==sizeof(int32_t)&&sizeof(jlong)==sizeof(int64_t));
int32_t int32(jint value) {return static_cast<int32_t>(value);}
const float* camera_data(JNIEnv* env,jobject camera,int64_t& floats) {
    auto* data=static_cast<const float*>(env->GetDirectBufferAddress(camera));
    floats=data?static_cast<int64_t>(env->GetDirectBufferCapacity(camera)):0;
    return data;
}
}
#define JNI_METHOD(name) extern "C" JNIEXPORT name
JNI_METHOD(jlong) JNICALL Java_org_kpc_rescaleframecraft_render_NativeBridge_createSurface(JNIEnv*,jclass,jlong parent) {
    return handle(rsf_minecraft_create_surface(platform(),window(parent)));
}
JNI_METHOD(void) JNICALL Java_org_kpc_rescaleframecraft_render_NativeBridge_destroySurface(JNIEnv*,jclass,jlong surface) {
    rsf_minecraft_destroy_surface(platform(),window(surface));
}
JNI_METHOD(jlong) JNICALL Java_org_kpc_rescaleframecraft_render_NativeBridge_open(JNIEnv* env,jclass,jlong hwnd,jlong surface,jstring root) {
    Text path(env,root);
    return handle(rsf_minecraft_open(platform(),window(hwnd),window(surface),path.text));
}
JNI_METHOD(jintArray) JNICALL Java_org_kpc_rescaleframecraft_render_NativeBridge_configure(JNIEnv* env,jclass,jlong pointer,jint width,jint height,jint sr,jint quality,jint preset,jint fg,jint count,jint reflex,jint limit,jboolean debug,jfloatArray sharpening) {
    if(!sharpening||env->GetArrayLength(sharpening)!=static_cast<jsize>(RSF_MINECRAFT_SHARPENING_FIELDS))return nullptr;
    rsf_minecraft_configuration configuration{int32(width),int32(height),int32(sr),int32(quality),int32(preset),int32(fg),
        int32(count),int32(reflex),int32(limit),debug?1u:0u,{}};
    env->GetFloatArrayRegion(sharpening,0,static_cast<jsize>(RSF_MINECRAFT_SHARPENING_FIELDS),configuration.sharpening);
    if(env->ExceptionCheck())return nullptr;
    std::array<int32_t,RSF_MINECRAFT_SURFACE_FIELDS> values{};
    if(!rsf_minecraft_configure(platform(),pipeline(pointer),&configuration,values.data()))return nullptr;
    auto array=env->NewIntArray(static_cast<jsize>(values.size()));
    if(array)env->SetIntArrayRegion(array,0,static_cast<jsize>(values.size()),reinterpret_cast<const jint*>(values.data()));
    return array;
}
JNI_METHOD(void) JNICALL Java_org_kpc_rescaleframecraft_render_NativeBridge_cpu(JNIEnv*,jclass,jlong pointer,jlong frame,jint stage) {
    rsf_minecraft_cpu(platform(),pipeline(pointer),static_cast<uint64_t>(frame),static_cast<uint32_t>(stage));
}
JNI_METHOD(void) JNICALL Java_org_kpc_rescaleframecraft_render_NativeBridge_input(JNIEnv*,jclass,jlong pointer,jlong frame,jint kinds) {
    rsf_minecraft_input(platform(),pipeline(pointer),static_cast<uint64_t>(frame),static_cast<uint32_t>(kinds));
}
JNI_METHOD(void) JNICALL Java_org_kpc_rescaleframecraft_render_NativeBridge_abort(JNIEnv*,jclass,jlong pointer,jlong frame) {
    rsf_minecraft_abort(platform(),pipeline(pointer),static_cast<uint64_t>(frame));
}
JNI_METHOD(jint) JNICALL Java_org_kpc_rescaleframecraft_render_NativeBridge_evaluate(JNIEnv* env,jclass,jlong pointer,jlong frame,jobject camera,jboolean reset,jint screen) {
    int64_t floats=0;const auto* data=camera_data(env,camera,floats);
    return rsf_minecraft_evaluate(platform(),pipeline(pointer),static_cast<uint64_t>(frame),data,floats,reset?1u:0u,int32(screen));
}
JNI_METHOD(jint) JNICALL Java_org_kpc_rescaleframecraft_render_NativeBridge_hudless(JNIEnv* env,jclass,jlong pointer,jlong frame,jobject camera) {
    int64_t floats=0;const auto* data=camera_data(env,camera,floats);
    return rsf_minecraft_hudless(platform(),pipeline(pointer),static_cast<uint64_t>(frame),data,floats);
}
JNI_METHOD(jint) JNICALL Java_org_kpc_rescaleframecraft_render_NativeBridge_present(JNIEnv*,jclass,jlong pointer,jlong frame,jboolean vsync) {
    return rsf_minecraft_present(platform(),pipeline(pointer),static_cast<uint64_t>(frame),vsync);
}
JNI_METHOD(void) JNICALL Java_org_kpc_rescaleframecraft_render_NativeBridge_status(JNIEnv* env,jclass,jlong pointer,jlongArray destination) {
    if(!destination)return;
    const auto capacity=env->GetArrayLength(destination);
    std::array<int64_t,RSF_MINECRAFT_STATUS_FIELDS> values{};
    const auto count=rsf_minecraft_status(platform(),pipeline(pointer),capacity,values.data());
    if(count)env->SetLongArrayRegion(destination,0,static_cast<jsize>(count),reinterpret_cast<const jlong*>(values.data()));
}
JNI_METHOD(jint) JNICALL Java_org_kpc_rescaleframecraft_render_NativeBridge_close(JNIEnv*,jclass,jlong pointer) {
    return rsf_minecraft_close(platform(),pipeline(pointer));
}

/* SPDX-License-Identifier: MIT */
#pragma once
/* Internal boundary between the typed JNI leaves (jni.cpp) and their Rust owner (lib.rs). Not a
   DLL ABI: both sides link into ReScaleFrame.Minecraft.dll next to the same OrchestratorCore.
   tools/generate-rust-bindings.py generates native/platform_abi.rs from this header. */
#include <stdbool.h>
#include <stdint.h>
#include <rescaleframe/gl_pipeline.h>
#include <rescaleframe/native_cpu.h>

#define RSF_MINECRAFT_CAMERA_FLOATS 88u
#define RSF_MINECRAFT_SHARPENING_FIELDS 12u
#define RSF_MINECRAFT_SURFACE_FIELDS 10u
/* NativeBridge.status callers that predate the FG limits pass 14 slots; current callers pass 25. */
#define RSF_MINECRAFT_STATUS_MINIMUM 14u
#define RSF_MINECRAFT_STATUS_FIELDS 25u
/* RSF_SCREEN_FLIGHT; jni.cpp asserts the match. */
#define RSF_MINECRAFT_HUDLESS_SCREEN 4u

#ifdef __cplusplus
extern "C" {
#endif

/* The linked Runtime entries and the C++-only sharpening helpers. jni.cpp fills every entry from
   the same OrchestratorCore it links; the Rust owner never resolves a second Runtime. */
typedef struct rsf_minecraft_platform {
    void* (*window_create)(void* parent);
    int (*window_destroy)(void* window);
    rsf_backend_result (*open)(const rsf_gl_pipeline_setup* setup, rsf_gl_pipeline** pipeline);
    rsf_backend_result (*configure)(rsf_gl_pipeline* pipeline, const rsf_gl_pipeline_options* options,
        uint32_t width, uint32_t height);
    rsf_backend_result (*get_surfaces)(rsf_gl_pipeline* pipeline, rsf_gl_pipeline_surfaces* surfaces);
    void (*cpu)(rsf_gl_pipeline* pipeline, uint64_t frame, uint32_t stage);
    int32_t (*input)(rsf_gl_pipeline* pipeline, uint64_t frame, uint32_t kinds, uint32_t message);
    rsf_backend_result (*abort)(rsf_gl_pipeline* pipeline, uint64_t frame);
    rsf_backend_result (*evaluate)(rsf_gl_pipeline* pipeline, const rsf_game_render_pass* pass);
    rsf_backend_result (*hudless)(rsf_gl_pipeline* pipeline, const rsf_game_render_pass* pass);
    rsf_backend_result (*present)(rsf_gl_pipeline* pipeline, uint64_t frame, uint32_t vsync);
    rsf_backend_result (*status_get)(rsf_gl_pipeline* pipeline, rsf_gl_pipeline_status* status);
    rsf_backend_result (*close)(rsf_gl_pipeline* pipeline);
    void (*input_dump_configure)(const char* directory, const char* game, const char* executable_sha256,
        rsf_dump_log_fn log, void* user);
    int (*latency_status_get)(rsf_native_latency_status* status);
    /* rsf_sharpening_defaults and rsf::sharpening_settings::valid are header-inline C/C++. */
    void (*sharpening_defaults)(rsf_sharpening_options* options);
    bool (*sharpening_valid)(const rsf_sharpening_options* options);
} rsf_minecraft_platform;

/* NativeBridge.configure arguments after JNI marshaling. sharpening is TaaSettings.sharpening(). */
typedef struct rsf_minecraft_configuration {
    int32_t width, height, sr, quality, preset, fg, count, reflex, limit;
    uint32_t debug;
    float sharpening[RSF_MINECRAFT_SHARPENING_FIELDS];
} rsf_minecraft_configuration;

/* A message the GLFW owner thread removed from its queue. */
typedef void (*rsf_minecraft_message_fn)(uint32_t message);

/* Rust owner entry points, one per NativeBridge native method. platform must be complete and live
   for the process; the first accepted table is kept. */
void* rsf_minecraft_create_surface(const rsf_minecraft_platform* platform, void* parent);
void rsf_minecraft_destroy_surface(const rsf_minecraft_platform* platform, void* window);
/* runtime_directory_utf8 is borrowed for the call; null refuses. */
rsf_gl_pipeline* rsf_minecraft_open(const rsf_minecraft_platform* platform, void* hwnd, void* surface,
    const char* runtime_directory_utf8);
/* Fills the ten NativeBridge.configure result values; false is a null Java result. */
bool rsf_minecraft_configure(const rsf_minecraft_platform* platform, rsf_gl_pipeline* pipeline,
    const rsf_minecraft_configuration* configuration, int32_t* surfaces);
void rsf_minecraft_cpu(const rsf_minecraft_platform* platform, rsf_gl_pipeline* pipeline, uint64_t frame,
    uint32_t stage);
void rsf_minecraft_input(const rsf_minecraft_platform* platform, rsf_gl_pipeline* pipeline, uint64_t frame,
    uint32_t kinds);
void rsf_minecraft_abort(const rsf_minecraft_platform* platform, rsf_gl_pipeline* pipeline, uint64_t frame);
/* camera is the direct FloatBuffer address and camera_floats its capacity; a short or missing
   buffer submits an empty pass exactly as before. */
int32_t rsf_minecraft_evaluate(const rsf_minecraft_platform* platform, rsf_gl_pipeline* pipeline, uint64_t frame,
    const float* camera, int64_t camera_floats, uint32_t reset, int32_t screen);
int32_t rsf_minecraft_hudless(const rsf_minecraft_platform* platform, rsf_gl_pipeline* pipeline, uint64_t frame,
    const float* camera, int64_t camera_floats);
int32_t rsf_minecraft_present(const rsf_minecraft_platform* platform, rsf_gl_pipeline* pipeline, uint64_t frame,
    uint32_t vsync);
/* Fills up to RSF_MINECRAFT_STATUS_FIELDS values and returns how many of them the caller's array
   of length capacity receives; zero leaves it untouched. */
uint32_t rsf_minecraft_status(const rsf_minecraft_platform* platform, rsf_gl_pipeline* pipeline, int64_t capacity,
    int64_t* values);
int32_t rsf_minecraft_close(const rsf_minecraft_platform* platform, rsf_gl_pipeline* pipeline);

/* WH_GETMESSAGE observer on the GLFW owner thread. Install on the parent's owning thread only;
   it reports PM_REMOVE messages for the parent, the child or the thread. One observer at a time. */
bool rsf_minecraft_observer_install(void* parent, void* child, rsf_minecraft_message_fn sink);
void rsf_minecraft_observer_close(void);

#ifdef __cplusplus
}
#endif

/* SPDX-License-Identifier: MIT */
/* The AC7 carrier's native reporting: the game controller's passes, CPU events and Present
   reach the core D3D11 host (rescaleframe/d3d11_host.h), which owns every technique. What stays
   here is AC7 input preparation: motion capture, the velocity pass, the frame tap that feeds
   them, and the research route dump. Not a public interface. */

#ifndef RSF_DLSS_BRIDGE_H
#define RSF_DLSS_BRIDGE_H

#include <rescaleframe/d3d11_observer.h>
#include <rescaleframe/game_api.h>

typedef void (*rsf_bridge_log_fn)(void* user, const char* message);

/* Unreal's velocity encoding, from Common.ush: In * (0.499 * 0.5) + 32767/65535. Written here as
   the decode a backend needs, which is the reciprocal of that scale and the same bias. The sentinel
   is far outside any real screen space motion and still exact in half precision. */
#define RSF_UNREAL_MOTION_SCALE (1.0f / (0.499f * 0.5f))
#define RSF_UNREAL_MOTION_BIAS (32767.0f / 65535.0f)
#define RSF_MOTION_SENTINEL (-1000.0f)

/* Streamline identity of the game this proxy is built for (Ace Combat 7, Unreal Engine 4.18.3).
   ENGINE_TYPE is sl::EngineType::eUnreal. */
#define RSF_AC7_ENGINE_TYPE 1u
#define RSF_AC7_ENGINE_VERSION "4.18.3"
#define RSF_AC7_PROJECT_ID "a3ed1f08-3542-4698-b85c-e1a9908e861a"

/* The core host's services, kept for the game controller and the Present callback. */
void rsf_bridge_set_host(const rsf_game_host_services* host);

/* Native game-controller preparation, before graphics activation. Its passes and CPU events go
   to the host; scope facts and the velocity commit stay here. */
int rsf_bridge_prepare_game(const wchar_t* plugin_path, const rsf_game_probe* probe);

/* The host's adapter Present callback. Binds the presentation, starts the native renderer once
   the decrypted image is patched, and does AC7's per-frame input work. */
void rsf_bridge_present(void* user, const rsf_observer_present_event* event);

void rsf_bridge_set_log(rsf_bridge_log_fn log, void* log_user);

/* Resource creation hooks for the observer: input layouts feed the motion capture and the velocity
   pass, shaders the motion capture. */
rsf_observer_layout_fn rsf_bridge_layout_hook(void);
rsf_observer_shader_fn rsf_bridge_shader_hook(void);

/* Whether the motion capture is configured, so the constant watch it needs stays armed. */
void rsf_bridge_set_motion_capture(int on);
/* The patched temporal jitter gate. The native renderer supplies its own jitter, so the gate
   is shut when it starts; the panel can override. */
void rsf_bridge_set_jitter_control(void (*set_jitter)(unsigned long open));

/* Ask for the native route of the next frames to be written out. */
void rsf_bridge_request_dump(const char* prefix);
int rsf_bridge_running(void);

#endif /* RSF_DLSS_BRIDGE_H */

// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/d3d11_host.h>
#include <cstdio>
#include <limits>
namespace {
unsigned errors{}, cpu_calls{}, logs{};
int cookie{};
void check(bool ok, const char* message) { if (!ok) { ++errors; std::printf("FAIL %s\n", message); } }
void cpu(void* user, const rsf_game_cpu_event* event) { check(user == &cookie && event->session_id == 91, "CPU upstream user and event preserved"); ++cpu_calls; }
void log(void* user, const char*) { check(user == &cookie, "Log upstream user preserved"); ++logs; }
}
int main() {
    rsf_d3d11_host_setup setup{}; setup.struct_size = sizeof(setup); setup.abi_version = RSF_D3D11_HOST_ABI_VERSION;
    setup.configuration_path = L"unused.ini"; setup.configuration_section = L"Test"; setup.runtime_directory = L"unused";
    setup.identity_utf8 = "contract"; setup.executable_sha256 = "unused"; setup.engine_version_utf8 = "1";
    setup.project_id_utf8 = "668392a0-ecfe-4d0c-9f58-c2838c5ca308";
    setup.units_to_meters = 1; setup.quality_choices = 1;
    setup.motion.struct_size = sizeof(setup.motion); setup.motion.scale_x = setup.motion.scale_y = 1;
    rsf_game_host_services host{sizeof(host), RSF_GAME_ABI_VERSION, 91, log, nullptr, &cookie, nullptr, cpu};
    check(rsf_d3d11_host_create(nullptr, &host) == RSF_ERROR_INVALID_ARGUMENT, "Null setup refused");
    --setup.struct_size; check(rsf_d3d11_host_create(&setup, &host) == RSF_ERROR_INVALID_ARGUMENT, "Short setup refused"); ++setup.struct_size;
    ++setup.abi_version; check(rsf_d3d11_host_create(&setup, &host) == RSF_ERROR_ABI_MISMATCH, "Setup ABI checked"); --setup.abi_version;
    --host.struct_size; check(rsf_d3d11_host_create(&setup, &host) == RSF_ERROR_INVALID_ARGUMENT, "Short services refused"); ++host.struct_size;
    ++host.abi_version; check(rsf_d3d11_host_create(&setup, &host) == RSF_ERROR_ABI_MISMATCH, "Services ABI checked"); --host.abi_version;
    setup.units_to_meters = std::numeric_limits<float>::quiet_NaN();
    check(rsf_d3d11_host_create(&setup, &host) == RSF_ERROR_INVALID_ARGUMENT, "Nonfinite units refused"); setup.units_to_meters = 1;
    setup.motion.scale_x = 0; check(rsf_d3d11_host_create(&setup, &host) == RSF_ERROR_INVALID_ARGUMENT, "Zero decode scale refused"); setup.motion.scale_x = 1;
    setup.quality_choices = 0; check(rsf_d3d11_host_create(&setup, &host) == RSF_ERROR_INVALID_ARGUMENT, "Empty capabilities refused"); setup.quality_choices = 1;
    auto upstream = host;
    check(rsf_d3d11_host_create(&setup, &host) == RSF_OK, "Graphics-free creation succeeds without SDK directories");
    check(host.session_id == 91 && host.user != &cookie && host.process_upscale && host.render_config && host.render_feedback &&
        !host.present && !host.submit && !host.process_latency, "Only admitted host services published; no pacing or generation");
    check(rsf_d3d11_host_create(&setup, &upstream) == RSF_ERROR_BUSY, "Second process owner refused");
    check(rsf_d3d11_host_set_active(&cookie, 1) == RSF_ERROR_INVALID_ARGUMENT, "Only the returned owner controls activation");
    check(rsf_d3d11_host_set_active(host.user, 1) == RSF_OK && rsf_d3d11_host_set_active(host.user, 0) == RSF_OK, "Thread-safe activation and suspension");
    rsf_game_cpu_event event{}; event.struct_size = sizeof(event); event.session_id = 91;
    host.cpu_event(host.user, &event); host.log(host.user, "contract");
    check(cpu_calls == 1 && logs == 1, "Callbacks forwarded exactly once");
    rsf_game_render_config config{sizeof(config)};
    check(host.render_config(host.user, &config) == 1 && !config.enabled && !config.output_width, "Cold config does not invent graphics dimensions");
    check(host.process_upscale(host.user, nullptr, nullptr) == RSF_ERROR_INVALID_ARGUMENT, "Null graphics lease refused");
    check(rsf_d3d11_host_request_capture(nullptr) == RSF_OK, "Any-thread capture request does no graphics work");
    return errors ? 1 : 0;
}

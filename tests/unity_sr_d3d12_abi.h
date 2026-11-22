/* SPDX-License-Identifier: MIT */
#pragma once
#include <rescaleframe/native_sr_d3d12.h>
#include <rescaleframe/native_fg_d3d12.h>
#include <rescaleframe/native_cpu.h>
#include <rescaleframe/render_frontend.h>
#include <rescaleframe/streamline_host.h>
#include <rescaleframe/unity_bridge.h>
#include "../games/unity-mono/include/rescaleframe/unity_runtime_host.h"
#include "../games/rdr2/src/runtime_setup.h"

/* Bindgen cannot evaluate the public headers' cast macros directly. */
enum rsf_fixture_unity_constants {
    RSF_FIXTURE_UNITY_ENGINE = RSF_DLSS_ENGINE_UNITY,
    RSF_FIXTURE_UNITY_INVALID = RSF_BACKEND_ERROR_INVALID_ARGUMENT,
    RSF_FIXTURE_UNITY_SIM_START = RSF_LATENCY_SIMULATION_START,
    RSF_FIXTURE_UNITY_SIM_END = RSF_LATENCY_SIMULATION_END,
    RSF_FIXTURE_UNITY_SUBMIT_START = RSF_LATENCY_RENDER_SUBMIT_START,
    RSF_FIXTURE_UNITY_SUBMIT_END = RSF_LATENCY_RENDER_SUBMIT_END,
    RSF_FIXTURE_UNITY_PRESENT_START = RSF_LATENCY_PRESENT_START,
    RSF_FIXTURE_UNITY_PRESENT_END = RSF_LATENCY_PRESENT_END,
    RSF_FIXTURE_UNITY_INPUT_SAMPLE = RSF_LATENCY_INPUT_SAMPLE
};

#ifdef __cplusplus
extern "C" {
#endif
int rsf_fixture_unity_dlss_preset_valid(uint32_t preset);
#ifdef __cplusplus
}
#endif

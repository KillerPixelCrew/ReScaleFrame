/* SPDX-License-Identifier: MIT */
#pragma once
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

typedef struct rsf_anomaly_plan_matrix { double values[16]; } rsf_anomaly_plan_matrix;
typedef struct rsf_anomaly_plan_extent { uint32_t width, height; } rsf_anomaly_plan_extent;
typedef struct rsf_anomaly_plan_vec2 { double x, y; } rsf_anomaly_plan_vec2;
typedef struct rsf_anomaly_plan_vec4 { double x, y, z, w; } rsf_anomaly_plan_vec4;
typedef struct rsf_anomaly_plan_resolutions {
    rsf_anomaly_plan_extent output, render, pda;
} rsf_anomaly_plan_resolutions;
typedef struct rsf_anomaly_plan_camera {
    rsf_anomaly_plan_matrix view, projection;
    rsf_anomaly_plan_vec2 jitter;
} rsf_anomaly_plan_camera;
typedef struct rsf_anomaly_plan_frame {
    rsf_anomaly_plan_camera world, first_person;
    rsf_anomaly_plan_resolutions sizes;
    uint64_t source_frame;
    uint8_t view;
    bool has_first_person, camera_cut, first_person_cut;
} rsf_anomaly_plan_frame;
typedef struct rsf_anomaly_plan_token {
    uint64_t source_frame, serial, owner;
    uint8_t view;
} rsf_anomaly_plan_token;
typedef struct rsf_anomaly_plan_camera_pair {
    rsf_anomaly_plan_camera current, previous;
    uint64_t previous_frame;
    bool has_previous;
} rsf_anomaly_plan_camera_pair;
typedef struct rsf_anomaly_plan_view {
    rsf_anomaly_plan_token token;
    rsf_anomaly_plan_resolutions sizes;
    rsf_anomaly_plan_extent screen_ui;
    rsf_anomaly_plan_matrix world, first_person;
    uint32_t presentation;
    bool has_first_person;
} rsf_anomaly_plan_view;
typedef struct rsf_anomaly_plan_key { uint64_t owner, visual, generation; } rsf_anomaly_plan_key;
typedef struct rsf_anomaly_plan_pose {
    rsf_anomaly_plan_matrix world;
    uint64_t palette_layout, deformation_layout;
    const rsf_anomaly_plan_matrix* bones;
    size_t bone_count;
    const rsf_anomaly_plan_vec4* constants;
    size_t constant_count;
    double fade;
    uint8_t domain, kind;
    bool has_deformation, has_detail;
} rsf_anomaly_plan_pose;
typedef struct rsf_anomaly_plan_sample {
    rsf_anomaly_plan_matrix world, bone1;
    double deformation_w, fade;
} rsf_anomaly_plan_sample;
typedef struct rsf_anomaly_plan_capture {
    void* owner;
    const void* current;
    const void* previous;
    uint32_t status;
} rsf_anomaly_plan_capture;

typedef struct rsf_anomaly_plan_fixture {
    bool (*resolutions)(const rsf_anomaly_plan_resolutions*, rsf_anomaly_plan_resolutions*);
    bool (*jitter)(const rsf_anomaly_plan_matrix*, rsf_anomaly_plan_vec2, rsf_anomaly_plan_extent, rsf_anomaly_plan_matrix*);
    bool (*multiply)(const rsf_anomaly_plan_matrix*, const rsf_anomaly_plan_matrix*, rsf_anomaly_plan_matrix*);
    bool (*inverse)(const rsf_anomaly_plan_matrix*, rsf_anomaly_plan_matrix*);
    bool (*transform)(const rsf_anomaly_plan_matrix*, rsf_anomaly_plan_vec4, rsf_anomaly_plan_vec4*);
    bool (*pixel_motion)(rsf_anomaly_plan_vec4, rsf_anomaly_plan_vec4, rsf_anomaly_plan_extent,
        rsf_anomaly_plan_vec2, rsf_anomaly_plan_vec2, rsf_anomaly_plan_vec2*);
    bool (*matrix_read)(const rsf_anomaly_plan_matrix*, size_t, size_t, double*);
    bool (*matrix_write)(rsf_anomaly_plan_matrix*, size_t, size_t, double);
    void* (*create)(void);
    void (*destroy)(void*);
    uint32_t (*begin)(void*, const rsf_anomaly_plan_frame*, rsf_anomaly_plan_token*);
    bool (*cameras)(void*, const rsf_anomaly_plan_token*, uint8_t, rsf_anomaly_plan_camera_pair*);
    bool (*plan)(void*, const rsf_anomaly_plan_token*, rsf_anomaly_plan_view*);
    rsf_anomaly_plan_capture (*capture)(void*, const rsf_anomaly_plan_token*, rsf_anomaly_plan_key, const rsf_anomaly_plan_pose*);
    void (*drop_capture)(void*);
    void (*sample)(const void*, rsf_anomaly_plan_sample*);
    bool (*retire)(void*, rsf_anomaly_plan_key);
    bool (*reset_view)(void*, uint8_t);
    void (*reset_all)(void*);
} rsf_anomaly_plan_fixture;

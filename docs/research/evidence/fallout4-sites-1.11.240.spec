# Fallout 4 AE 1.11.240.0 (Steam, SHA-256 fdcef37a...646072f8) sites for tools/derive-site-patterns.py.
# KIND NAME RVA [LEN]: LEN is how many bytes from the site the pattern must cover (the bytes the
# old exact-build guards checked there). games/fallout4/src/discovery.rs holds the derived patterns.

# CPU producer hooks.
code swap c33250 13
code keyboard 1675140 10
code mouse 16754d0 8
code idle c339d0 11
code input_poll 1672540 8
code consume 1673ae0 181
code player_controls ddbe50 9
code job_end bd6850 10
code wrapper c34eb0 8
code worker_join bd51c0 44
code worker_destructor bd72e0
code worker_entry bd7430
# CPU caller sites: the pattern starts at the call (or its argument load).
code swap_call c346ac 5
code join_call c346bb 5
code keyboard_call c3a98d 5
code mouse_call c3a96b 5
code idle_call c315b4 12
code main_poll_call c342ae 12
code consume_call c342d9 5
code wrapper_poll_call c34ec3 5
code worker_poll_call bd764e 5
code worker_consume_call c34ee7 5
code wrapper_call bd6940 5
code job_callback bd76d2 13
code player_controls_call 1673b69 11
code peek_1 c338d3 27
code peek_2 c33907 27
code peek_3 c33959 54
code peek_4 c339a9 28
# Input notification evidence.
code controls_events ddbffb 4
code controls_kind ddc44c 4
code enqueue_sequence 167465c 23
code device_enqueue_1 16778a2 5
code device_enqueue_2 16760df 5
code queue_device 1673d1c 4
code device_enqueue_3 1677a5f 5
code device_enqueue_4 1677b02 5
code queue_kind 1674205 4
code queue_append 16742a4 53
code gamepad_state 1677bb7 6

# Render hooks.
code dr_writer 1839ed0 10
code prepass 21f38b0 150
code outer_present 1818550 137
code world_begin 21f0ac0 43
code viewport_store 1839ec0 7
code imagespace 21fa200 10
code effects 2185420 16
code taa_query 2214fc0 27
# Render call sites.
code present_call c335e0 12
code prepass_call 21f9f69 15
code dr_call 21f9f2f 5
code world_call c382a0 5
code effects_call 21fa283 5
code hudless_call 21fa2c5 5
code imagespace_call_1 c383c2 5
code imagespace_call_2 c39d6a 5
code imagespace_call_3 c39e0e 5
code builder_call 182c02d 5
code setter_call 182c597 5
# Called functions.
code camera_update 21f30b0
code camera_read 182bf20 92
code builder 182d750
code temporal_update 182d580 13
code viewport_builder 1817ef0 68
# Render evidence.
code camera_reads 21f3294 66
code depth_camera 21f3b36 122
code depth_build 21f3db6 5
code world_ranges 21f0e25 44
code builder_viewport 182d9d9 84
code builder_scale 182da63 15
code builder_owner 182db0f 48
code temporal_size 182d647 10
code dr_slots 183985b 141
code dr_select 1839bc9 32
code dr_store 1839c3c 18
code dr_index 1839cb1 17
code dr_entry 1839e00 59
code dr_ratio 183a278 8
code dr_count 183a2d4 7
code dr_viewport 183a4a0 15
code shadow_writer 1817f97 215
code target_select 1823db7 34
code target_view 1823f14 9
code target_bind 182430d 21
code effects_size 21856df 41
code renderer_init 18a246 77
code frame_read 182e226 6
code delta_read c33a60 7
code jitter_store 165ba9f 45

# Screen queries.
code menu_open 1a80320 91
code freeze_background 1a80480 11
code freeze_pause 1a804a0 11
code camera_equals 102b5c0 27

# Creation.
code create_call 1824d40 5

# Globals.
data main 32dd130
data input_manager 3e75cb8
data event_queue 3273ae0
data controls 31f0b80
data worker 32a12f0
data worker_object 32a1320
data renderer_pointer 38e13a0
data renderer_object 3a25d10
data render_context 38e1398
data renderer_window 38e13b0
data present_renderer 3a25d00
data tls_index 3e73018
data state 3d871a0
data targets 2f4d750
data world_graph 32dd338
data delta 339f430
data imagespace_manager 3e5e4b0
data viewport_unit 2929458
data jitter_scale 29293f8
data depth_mode 3e62250
data near_interval 3e62190
data world_interval 2f97b30
data near_camera 3e622d0
data world_camera 3e62210
data ui 30e8930
data player_camera 30e6e58

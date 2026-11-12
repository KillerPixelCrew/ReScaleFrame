# SPDX-License-Identifier: GPL-3.0-only
extends SceneTree

func _initialize() -> void:
    var settings = load("res://settings.gd").new()
    var defaults: Dictionary = settings.to_options()
    assert(is_equal_approx(float(defaults.sharpening_mode),0.0))
    assert(is_equal_approx(float(defaults.sharpening_sharpness),0.4))
    assert(is_equal_approx(float(defaults.sharpening_mas),0.0))
    assert(is_equal_approx(float(defaults.sharpening_motion_sharpness),0.2))
    assert(is_equal_approx(float(defaults.sharpening_motion_threshold),0.0))
    assert(is_equal_approx(float(defaults.sharpening_motion_scale_limit),10.0))
    assert(is_equal_approx(float(defaults.sharpening_contrast_enabled),0.0))
    assert(is_equal_approx(float(defaults.sharpening_contrast),0.0))
    assert(is_equal_approx(float(defaults.sharpening_depth_scale),35.0))
    assert(is_equal_approx(float(defaults.sharpening_depth_bias),0.001))
    assert(is_equal_approx(float(defaults.sharpening_clamp_output),0.0))
    assert(is_equal_approx(float(defaults.sharpening_debug),0.0))
    settings.sharpening_mode = 3
    settings.sharpening_sharpness = 0.3333333333333333
    settings.sharpening_mas = 1
    settings.sharpening_motion_sharpness = 0.0
    settings.sharpening_motion_threshold = 33.333333333333336
    settings.sharpening_motion_scale_limit = 33.336666666666666
    settings.sharpening_contrast_enabled = 1
    settings.sharpening_contrast = 0.6666666666666666
    settings.sharpening_depth_scale = 200.0
    settings.sharpening_depth_bias = 0.3333333333333333
    settings.sharpening_clamp_output = 1
    settings.sharpening_debug = 1
    var expected: Dictionary = settings.to_options()
    assert(ResourceSaver.save(settings,"res://sharpening.tres") == OK)
    var restored = ResourceLoader.load("res://sharpening.tres","",ResourceLoader.CACHE_MODE_IGNORE)
    assert(restored.to_options() == expected)
    print("PASS: Godot Resource saves and restores all 12 sharpening options and code defaults")
    quit(0)

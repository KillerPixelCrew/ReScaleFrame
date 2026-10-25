// SPDX-License-Identifier: GPL-3.0-only
using System;
using Godot;
using Godot.Collections;

namespace ReScaleFrame;

/// <summary>The same native compositor resource used by GDScript, including status reporting.</summary>
public static class GodotIntegration
{
    private static readonly StringName EffectClass = "ReScaleFrameEffect";
    private static readonly StringName StatusMethod = "get_status";

    private static RefCounted CreateRuntimeControl() => ClassDB.ClassExists("ReScaleFrameRuntime")
        ? ClassDB.Instantiate("ReScaleFrameRuntime").AsGodotObject() as RefCounted
            ?? throw new InvalidOperationException("ReScaleFrame's runtime facade could not be constructed.")
        : throw new InvalidOperationException("ReScaleFrame's native GDExtension is not loaded.");

    public static bool Configure(Dictionary options, Viewport viewport)
    {
        ArgumentNullException.ThrowIfNull(options);
        ArgumentNullException.ThrowIfNull(viewport);
        using var runtime = CreateRuntimeControl();
        return runtime.Call("configure", options, viewport.GetViewportRid()).AsBool();
    }

    public static Dictionary GetRuntimeStatus()
    {
        using var runtime = CreateRuntimeControl();
        return runtime.Call(StatusMethod).AsGodotDictionary();
    }

    public static void ResetHistory()
    {
        using var runtime = CreateRuntimeControl();
        runtime.Call("reset_history");
    }

    /// <summary>Posts a synthetic PCL message to validate marker plumbing, without measuring latency.</summary>
    public static bool RequestLatencyPing()
    {
        using var runtime = CreateRuntimeControl();
        return runtime.Call("request_latency_ping").AsBool();
    }

    public static CompositorEffect CreateEffect()
    {
        if (!ClassDB.ClassExists(EffectClass))
            throw new InvalidOperationException("ReScaleFrame's native GDExtension is not loaded.");
        return ClassDB.Instantiate(EffectClass).AsGodotObject() as CompositorEffect
            ?? throw new InvalidOperationException("ReScaleFrame's compositor could not be constructed.");
    }

    public static Dictionary GetStatus(CompositorEffect effect)
    {
        ArgumentNullException.ThrowIfNull(effect);
        if (!GodotObject.IsInstanceValid(effect) || !effect.IsClass(EffectClass))
            throw new ArgumentException("Expected a live ReScaleFrameEffect.", nameof(effect));
        return effect.Call(StatusMethod).AsGodotDictionary();
    }
}

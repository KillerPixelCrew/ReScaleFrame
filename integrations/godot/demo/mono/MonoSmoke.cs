// SPDX-License-Identifier: GPL-3.0-only
using Godot;
using Godot.Collections;
using ReScaleFrame;

public partial class MonoSmoke : Node
{
    private CompositorEffect? effect;

    public override void _Ready()
    {
        effect = GodotIntegration.CreateEffect();
        GetParent().Call("replace_effect", effect);
        if (Engine.HasSingleton("ReScaleFrame"))
        {
            bool vulkan = GodotIntegration.GetRuntimeStatus()["graphics_driver"].AsString() == "vulkan";
            var options = new Dictionary { ["sr_backend"] = vulkan ? 3 : 1, ["quality"] = 1, ["latency_mode"] = vulkan ? 0 : 1 };
            if (!GodotIntegration.Configure(options, GetViewport()))
                throw new System.InvalidOperationException("C# runtime configuration was refused.");
        }
    }

    public Dictionary validate_mono() => ValidateMono(true);

    public Dictionary validate_sr_mono() => ValidateMono(false);

    private Dictionary ValidateMono(bool requireLatency)
    {
        if (effect is null)
            return new Dictionary { ["passed"] = false };
        Dictionary status = GodotIntegration.GetStatus(effect);
        Dictionary runtime = Engine.HasSingleton("ReScaleFrame") ? GodotIntegration.GetRuntimeStatus() : new Dictionary();
        bool runtimePassed = runtime.Count == 0 || (runtime["evaluated_frames"].AsInt64() > 0
            && (!requireLatency || runtime["pcl_available"].AsInt32() != 0));
        return new Dictionary
        {
            ["passed"] = status["callbacks"].AsInt64() > 0 && status["buffers_available"].AsBool()
                && runtimePassed,
            ["created_from_csharp"] = true,
            ["callbacks"] = status["callbacks"],
            ["godot_version"] = Engine.GetVersionInfo()["string"],
            ["runtime_status"] = runtime
        };
    }
}

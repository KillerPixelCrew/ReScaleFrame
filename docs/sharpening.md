# Sharpening

Open the SR area's **Sharpening** section. Off is the default and preserves the
stock image. Changes apply live and save; overlay sliders apply when released.
Godot exposes the same values on its settings Resource, Unity URP on its Options
asset, and Minecraft under ReScaleFrame > Sharpening in Sodium's video settings.

Choose RCAS for ordinary sharpening, Depth RCAS or Depth DAS for depth-guided
filtering, or Vendor native for FSR's own sharpening. Vendor native requires an
active FSR2, FSR3 or FSR4 backend. DLSS and XeSS use the three shared filters.
Depth modes fall back to RCAS if depth is unavailable. Unused overlay controls
are hidden or disabled with a reason.

| Option | Default | What it does |
| --- | --- | --- |
| Mode | Off | Selects the filter. Off adds no GPU dispatch or copy. |
| Sharpness | 0.4 | Base strength, 0 to 1. Zero bypasses filtering when MAS is off. |
| MAS | Off | Adjusts sharpness according to motion. |
| Motion sharpness | 0.2 | Amount added at the motion limit; negative values soften moving areas. Range -1 to 1. |
| Motion threshold | 0 pixels | Motion below this speed keeps base sharpness. |
| Motion scale limit | 10 pixels | Speed where the full motion amount applies. Keep it above the threshold. |
| Contrast enabled | Off | Enables the RCAS contrast term. |
| Contrast amount | 0 | Adjusts the RCAS contrast term, from 0 to 2. |
| Depth scale | 35 | Makes depth rejection more sensitive as it increases. Depth modes only. |
| Depth bias | 0.001 | Tolerance before depth rejection begins. Depth modes only. |
| Clamp output | Off | Limits depth-mode output to 0 to 1. Leave off to retain HDR highlights. |
| Debug view | Off | Tints motion adjustments or depth rejection so their effect is visible. |

Filtering runs after reconstruction and before the result returns to the game:
D3D11 after color decode and fidelity, D3D12 before output copy, and Vulkan before
each backend's output copy, including FSR4's continuation command buffer.
Minecraft's GL path forwards these controls to its D3D12 SDR reconstruction.
HDR filtering uses exposure-aware perceptual encoding to preserve bright edges
and bounds ordinary output to the original local color range. Strong depth
breaks preserve the center pixel. Alpha is retained.

Unity URP's engine-owned spatial FSR1 keeps its separate **FSR1 Sharpness**
control because the engine owns that pass. The native temporal controls do not
add another filter to FSR1. Godot's engine-owned spatial FSR1 likewise uses the
engine's settings. Save engine settings through the editor's normal asset or
Resource workflow; Godot controllers apply a changed Resource with
`apply_settings()`. These controls do not require restarting the SR context.

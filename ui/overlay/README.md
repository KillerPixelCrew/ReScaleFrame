# In-game egui

This crate owns UI composition and will use the shared runtime settings/status contract. It currently draws a disconnected status window in a host-provided egui context.

Input integration, the native C boundary, DX11 rendering/state restoration, controller navigation, and MFG-aware UI composition are pending. The intended host is the game's existing window and graphics device, without a second swap chain. Pinning egui 0.35.0 keeps the initial frontend aligned with the researched DX11 renderer version.

# Vendor backends

Intel XeSS-SR/FG/MFG/XeLL, NVIDIA DLSS/FG/Reflex, and AMD FSR integrations belong here, behind the orchestrator's shared interface. No vendor SDK or runtime binary is bundled yet.

Keep capability checks specific to the API, adapter, driver, and requested feature. XeSS-SR supports Vulkan; the current native XeSS FG/MFG and XeLL path uses DX12. Only one backend may own frame generation and presentation for a session.

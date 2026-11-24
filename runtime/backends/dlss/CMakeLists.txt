# DLSS SR, the D3D12 Streamline host (streamline_host.rs), the RTX 40 MFG compatibility path
# and the Reflex API audit live in the Rust orchestrator. This target carries the public C headers
# and the archive that defines them. Only test fixtures still read the Streamline headers.
add_library(rsf_dlss INTERFACE)
target_include_directories(rsf_dlss INTERFACE include)
target_link_libraries(rsf_dlss INTERFACE ReScaleFrame::Contract rsf_orchestrator_rust d3d11 d3d12 dxgi)

add_library(ReScaleFrame::DLSS ALIAS rsf_dlss)

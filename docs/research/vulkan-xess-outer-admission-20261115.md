# XeSS admission through the shared SR entry point

The TotK 1.4.3 focused run on an RTX 4070 Laptop reached a real 1144x640
scene for XeSS's 1130x636 recommendation, then returned invalid argument before
the provider's input-range diagnostic. The game derives both scene axes from
width and floors them to its allocation grid; its adapter now requests an
achievable size. The previous failed run is retained outside the public tree.

Source inspection found that `native_sr_vulkan.cpp`, `rsf_srvk_evaluate`,
required the recommendation exactly before forwarding to `rsf_xessvk_evaluate`.
That prevented the XeSS service's existing queried minimum/maximum contract
from running. The shared entry point now validates XeSS images and camera
against the actual frame dimensions, then forwards to that provider validator.
DLSS and FFX retain their exact-plan checks. Identity, camera, format, output,
optional mask and packet-size validation remain in the shared entry point.

`tests/native_sr_xess_range_check.py` compiles the actual shared validation and
dispatch prefix with inert backend stubs. It checks forwarding of compatible
active extents, provider refusal propagation, malformed packets, mismatched
resources and camera, and the unchanged exact-plan checks for DLSS and FFX.
The clean native build and combined hardware run are coordinator-owned; this
CPU contract alone does not establish successful XeSS SDK execution.

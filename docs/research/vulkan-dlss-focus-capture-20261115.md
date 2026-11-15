# DLSS Vulkan presentation evidence and focus, 15 November 2026

TotK's first combined native-resolution DLSS SR/FG captures contained only real
presents, although the shared service had accepted preparation, HUD-less inputs
and On options. Successful configuration is not generated-output evidence.

The optional capture now retains raw SDK boundary facts during its requested
source interval, including unmatched boundaries. It records the SDK-generated
ordinal, boundary validity, source lookup result and a separately labeled
capture anchor. It also copies the previous SDK state query's result, status,
actual-present total, generated-count limit, minimum dimension and VSync/dynamic
capabilities. These are observations, not substituted frame identities. The
anchor is cleared outside the requested interval. An initial trace omitted that
clear and continued logging after its requested three frames; the correction
was built before the later visibility and focus controls.

The native DLSS owner updates that diagnostic snapshot before proxy Present,
after configuring the selected output source. Public C ABI and rendering policy
are unchanged. No new diagnostic option was introduced; it uses the existing
bounded Vulkan frame-capture request.

Background and raised-without-activation controls at sources 1800-1802 report
SDK result/status zero, On, one supported generated frame, minimum dimension
100, no VSync/dynamic-MFG support, and only ordinal -1. Actual-present totals
advance once per source. The bounded visible trace has three entries. Raising
the window without activation does not establish Windows foreground focus.

The existing verbose SDK option provides a direct rejection reason from the
signed NVIDIA plugin: `dlfgPresent.cpp`, `shouldInterpolate` reports that DLSS-G
is disabled because the window is not focused. TotK's assume-focus option keeps
its own simulation running; it does not change that SDK condition. A Win32
activation attempt was refused and recorded `focused: False`, so it is not a
focused control.

A subsequent computer-use activation visibly selected the game window. Its SDK
actual-present total then grew beyond its real source count: source 1500 reports
1517 and source 1800 reports 2055. The run completed 2541 real presents with zero
validation errors, shader failures or skipped draws. This supports focus-based
suppression as the earlier explanation, but remains a counter observation. The
requested sources 2100-2102 produced no image records; whether that interval
missed an accepted scene or another capture mapping needs correction is not
established. Generated-image acceptance is still pending. The user stopped
computer use with Escape, and UI actions stopped.

Logs and private readbacks remain under TotK's ignored
`build/checks/rsf-dlss-fg-boundary-*` and
`build/checks/rsf-dlss-fg-gui-focused-capture*`. The actual native DLSS synthetic
contract passes. The trace change's full Release gate ran 232 tests with the
two recorded Anomaly FG device failures; Rust formatting and Clippy pass
independently. Concurrent FSR4 and Skyrim source changes require their own
current-tree verification. This note does not claim a measured frame-rate or
latency improvement, or image/resize/lifecycle acceptance.

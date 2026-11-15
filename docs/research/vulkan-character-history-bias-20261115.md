# Vulkan character history bias

An input audit of the TotK integration found a dropped character history hint.
The user's skin-smearing report and F10 capture were subsequently identified as
belonging to Skyrim; they are not evidence of a TotK defect. The TotK renderer's
`runtime/vk_dlaa.c`, `mv_pass` already produces an R8 current-colour bias mask
from character classes in the G-buffer, including reprojected previous coverage.
Its default strength is 0.65. The migration to `rsf_srvk_frame` omitted that mask.
Source review also found that `native_dlss_vulkan.cpp`, `evaluate` tagged only
colour, depth, motion and output, so forwarding a material-reactivity mask alone
would not repair this DLSS input.

The Vulkan frame contract now appends an optional `bias_current_color` image.
It retains ABI 2 and accepts the old complete frame prefix, treating an absent
extension as no mask. The service validates render dimensions, device, R8 format
and sampled layout. The DLSS owner requires transfer-source usage, copies into
its own sampled image and submits `kBufferTypeBiasCurrentColorHint` with the
same source token and viewport as colour/depth/motion. An explicit null tag clears
the hint when no mask is supplied. The hint stays separate from reactive and
transparency masks; other providers retain their existing inputs.

The engine retains its mask through GPU completion and restores GENERAL layout
after recording, including failed/pending evaluations. A missing current G-buffer
or disabled mask never submits stale mask contents. This repairs a verified
missing binding; it does not establish complete skin deformation motion coverage
or prove that this was the user's only source of smearing.

An initial TotK opening-scene run completed but exposed Streamline format errors:
Streamline 2.14.1, `source/platforms/sl.chi/vulkan.cpp`, `Vulkan::getFormat`
omits R8_UNORM although `getNativeFormat` supports the opposite mapping.
The DLSS owner therefore blits the R8 hint into its own R16F image before tagging.
The public R8 contract and mask values remain unchanged.

The corrected run (`build/checks/rsf-skin-game-fixed.log` in the TotK workspace)
completed 1,767 presents and at least 300 successful SR evaluations on the RTX
4070 Laptop GPU, with no Streamline format errors, Vulkan validation errors,
shader failures or skipped draws. Validation used GPL off and descriptor writes;
this does not establish default GPL/template-path acceptance or skin quality.

The updated native Vulkan SR device fixture covers the old frame size, adding
and removing the hint, invalid format and foreign-device rejection, but its device
cases were skipped by the full gate without provider environment variables.
Four focused Vulkan ABI/split contract tests pass after the final changes.
`eng/verify.ps1 -Configuration Release -VS2026` built and ran 231 tests; the
existing `anomaly_fg_dlss_sr5_device` and `anomaly_fg_fsr3_sroff_device` failures
prevented a green full gate and the subsequent Rust checks. Moving-scene visual
acceptance remains separate from ABI/device validation.

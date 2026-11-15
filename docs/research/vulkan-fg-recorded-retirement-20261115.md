# Retain Vulkan FG owners for unsubmitted recordings

## Question and discovery

Connecting TotK's scene commands to FG exposed a retirement gap. A device-idle
wait only covers submitted work. The host may still hold a command buffer that
references SDK preparation resources or private input images, even after an SDK
call reports failure. Destroying the provider at that point leaves stale GPU
references in the host's next submission.

The existing per-slot recorded/submitted state already distinguishes this case.
FSR 3 and DLSS track recorded preparation and HUD-less copying separately from
successful tagging. The FSR 4 and XeSS Vulkan bridges track their private copies
as prepared and HUD-less-recorded before submission. Each provider now refuses
drain while either role has an outstanding recording. Destroy also retains the
owner. FSR 3 checks before configuring Off; DLSS checks before invoking disable.
The host submits and reports that work, then retries retirement after completion.
No game policy is involved, and no implicit abandonment is introduced.

## Evidence and use

The actual FSR 3 owner fixture checks that a partially failed prepare retains its
owner and that submitting guides alone cannot retire a later HUD-less recording.
The actual DLSS owner fixture checks rejected tags with both recorded copies,
verifies drain/destroy refuse before publishing Off, then submits the work and
observes a successful drain. Both synthetic Windows Release fixtures pass.
Bridge changes compile with their existing native targets; that is not bridge
device or lifecycle acceptance.

TotK records preparation at its existing depth/motion producer and supplies its
copied frame/camera metadata. When FG is selected, `runtime/vk_rec.c` `rec_submit`
waits for the recorder's actual submission result and notifies the presentation
owner on the producer, outside the Vulkan queue lock. Direct submissions follow
the same rule. The recorder never takes the presentation lock: Present may hold
it while waiting for recorder queue ordering. This temporarily reduces CPU
overlap for FG and needs later performance measurement. SDK-owned submission
fences still establish GPU completion; host notification alone is not completion.

TotK's swapchain retirement already preserves an owner on a failed drain, so a
resize cannot destroy an SDK referenced by an unsubmitted preparation. HUD-less
capture remains unconnected, and per-output admission keeps interpolation off.

The full Release gate ran 232 tests and retains the two recorded Anomaly FG
device failures, `anomaly_fg_dlss_sr5_device` and
`anomaly_fg_fsr3_sroff_device`. Rust formatting and Clippy pass independently.
TotK self-test, configuration and Vulkan execution probes pass. Bounded
FG-only opening-scene runs accepted SDK preparation and exited normally with
1,690 FSR 3 presents and 1,679 DLSS presents. A combined native-resolution DLSS
run completed 1,798 presents and at least 300 successful SR evaluations plus
300 accepted FG preparations. All three game runs reached the world with zero
Vulkan validation errors, shader failures or skipped draws, using GPL off and
descriptor writes on the RTX 4070 Laptop GPU. Interpolation stayed off; these
are preparation/submission checks, not generated-image acceptance or a full
resize/teardown test. Logs are in TotK's ignored `build/checks/rsf-fg-prepare-*`
and `build/checks/rsf-fg-pending-*` files.

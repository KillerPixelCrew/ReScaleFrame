# ReScaleFrame for Ace Combat 7: current development package

This is a package of current source, separate from the published v0.1.0 SR-only
release. The manifest identifies its source commit and file hashes. It includes
the dedicated AC7 carrier, game plugin, overlay, SR runtimes and the DLSS-G,
FidelityFX FG and XeSS FG/XeLL dependencies. No original game files are included.

Close AC7. Back up any existing mod files, then extract the entire package beside
`Ace7Game.exe`. If another mod owns `dinput8.dll`, resolve that conflict before
replacing it. Launch through Steam and press Insert for the overlay.

SR and FG are separate controls. FSR4 **frame generation** stays visible but is
greyed out when the loaded SDK exposes no major-4 FG provider for the owning
device. Supported devices retain the choice. FSR4 INT8 **upscaling** is separate.
FG provider changes apply during play; generated-frame counts are SDK-limited.
Fresh defaults keep FG Off; saved preferences can select a provider at startup.

Preferences: `%LOCALAPPDATA%/ReScaleFrame/AC7.ini`.
Logs: `%LOCALAPPDATA%/ReScaleFrame/AC7/rsf-dump.log`.
The sample INI contains portable defaults and optional diagnostics.

Native, overlay and SDK fixtures validate particular paths; this new package has
not received a fresh AC7 gameplay test. Refer to the implementation tracker and
research for the recorded acceptance and remaining hardware limits.

To uninstall, close AC7 and remove the added DLLs, INI and ReScaleFrame folder,
restoring any backed-up files. Licenses and notices are included in `licenses/`
and beside the vendor runtimes. Corresponding source accompanies this package.

# ReScaleFrame for Project Wingman: observer preview

This Windows x64 development package records the native temporal render graph.
It does not implement upscaling, frame generation, latency integration or VR
processing. It is intended for renderer research, not a visual upgrade.

Requires the researched Steam build 16671873 and the x64 Visual C++ v14 runtime.
Executable: `ProjectWingman-Win64-Shipping.exe`, SHA256
`092e84225624a4de9c05d2404ff55269bd4a2aa2ff0548f2a183c6bf36abc85a`.
Other builds are refused. Install the Microsoft runtime from
https://aka.ms/vc14/vc_redist.x64.exe if MSVCP140/VCRUNTIME140 is missing.

1. Close Project Wingman. Back up existing mod files.
2. Extract the entire ZIP beside `ProjectWingman-Win64-Shipping.exe`.
   `xinput1_3.dll` and `ReScaleFrame.ini` belong beside the executable, with the
   included `ReScaleFrame` directory. Do not replace another mod's XInput proxy;
   chaining is not supported by this package.
3. Launch normally. The included `[ProjectWingman] ObserveNative=1` setting
   enables the guarded observer. Native TAA continues unchanged.
4. Read `ReScaleFrame/project-wingman.log` for prepare/start results and graph facts.

Set `ObserveNative=0` and restart to disable observation. There is no settings
overlay or vendor-runtime payload in this observer package. The runtime only
uses the game-plugin lifecycle here and does not activate a graphics provider.

The manifest identifies the source commit, exact file hashes and capability
limits. Build/contract tests do not establish a fresh live-game or VR test.
Earlier desktop acceptance is recorded in the repository's Project Wingman
guide and research; this new package has not received a fresh gameplay test.

To uninstall, close the game and remove the added `xinput1_3.dll`,
`ReScaleFrame.ini` and `ReScaleFrame` directory, restoring backed-up mod files.
Include the source commit, game build and observer log with issue reports.
First-party code is GPL-3.0-only except the MIT Game SDK. Included MinHook and
MFG-Unlocker notices retain their respective terms. Corresponding source is
provided as a separate archive.

# Anomaly C++ adapter baseline on the Rust runtime

Revision `8322179a36c7` was built in MSVC Release with the thin DirectInput
carrier, Rust-backed Runtime, existing C++ adapter, panel and motion rules.
The five staged files were hash-checked before copying. The Standard and AVX
executables in the isolated Anomaly 1.5.3 clone matched the hashes in
[`engine.json`](../../games/anomaly/engine.json). The clone used its own copied
save, configuration, logs and shader cache. The installed game was left intact.
The device was an RTX 4070 Laptop GPU.

## Observed in the game

| Profile | Observed behavior |
| --- | --- |
| Standard | The thin carrier entered the guarded adapter before device creation. The copied save loaded into the world and DLSS Quality evaluated 1067×600 inputs for 1600×900 output. The overlay applied Balanced at 928×522, disabled SR for native rendering, then enabled it and switched to FSR3 Balanced at 941×529. The final pipeline count was 22,523 accepted evaluations with zero refusals. |
| Standard input and presentation | Insert opened and closed the panel, and its trace drew all 804 visible frames in the first opening. The panel applied and saved options. DLSS-G advanced from off to active 2× and back to off, with the HUD showing about 58 rendered and 117 presented per second while active. After closing the overlay, a held W key moved the camera. A held Escape opened the menu. |
| Standard sharpening | RCAS, Depth RCAS and Depth DAS were selected live in the same indoor view. Both depth modes retained the visual color of the fixed scene at sharpness 0.75, with Clamp Output on and Debug View off. These are side-by-side window observations. They do not establish matched shader-input chromaticity in other scenes. |
| AVX | The guarded AVX profile loaded the same save through the engine startup command. DLSS Quality and DLSS-G 2× were active in the world. A game video restart changed 1600×900 output to 1280×720, rebuilt the targets once and resumed DLSS at 853×480 with FG active. The final counter recorded 10,409 accepted evaluations, zero refusals and one successful resize. |
| Exit | Both processes exited after the engine's `quit` command. The engine logs include level destruction and Direct3D teardown. Explicit plugin-session unload and complete COM retirement were not measured. |

The game's console and gameplay keys required held scancodes to establish
input consumption. The test checked that the foreground belonged to the isolated
process before submitting a key. [Windows keyboard input reference](https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-keybdinput)

Artifacts are under `.local/anomaly/staging/live-8322179a36c7/evidence/`.
The Standard folder contains window captures for each sharpening mode, movement,
FG activation and refusal-free settings changes, as well as eight game
backbuffer captures from the panel open and close events. The AVX folder
contains before/after resize images and separate engine and adapter logs.

In the same 270×330 wall region outside the overlay in the three Standard
window captures, mean RGB was (28.003, 22.824, 20.776) for RCAS,
(28.010, 22.817, 20.750) for Depth RCAS and (28.345, 23.097, 21.001)
for Depth DAS. Mean B/R was 0.74192, 0.74081 and 0.74092, respectively.
These are display-pixel samples from separate frames in one fixed view.

## Device and focused evidence

The Release D3D11 `sharpening` and D3D12 `sharpening_d3d12` fixtures passed.
The D3D11 run exercised the colored HDR ramp and output-clamp cases added
for the depth modes. Both tests ran outside the game.

The staged branch passed the Anomaly SR API, lifecycle, bootstrap and Standard
and AVX input-event CPU contracts. The earlier combined build passed 216
locked Rust tests and preserved all four recorded PE export inventories.

The game run covered returning to native rendering with SR disabled. A
forced vendor refusal after reduced rendering, PDA and scope paths and
other presentation providers still need separate game evidence. The
Rust adapter must repeat these checks after its own cutover.

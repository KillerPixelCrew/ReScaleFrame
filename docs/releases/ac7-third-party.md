# Package notices

ReScaleFrame's first-party implementation is GPL-3.0-only. Its public Game SDK header is MIT.
The corresponding texts are included here. Source for this release is available at
https://github.com/KillerPixelCrew/ReScaleFrame/tree/v0.1.0 and in the release's source archive.

The package also contains these separately licensed components:

- NVIDIA Streamline 2.14.1: the interposer, common, DLSS and PCL plugins, with its license and
  third-party notices included here.
- NVIDIA DLSS runtime 310.9.1.0: distributed unmodified under the NVIDIA RTX SDK license in
  `../streamline/nvngx_dlss.license.txt`. NVIDIA's proprietary runtime is not relicensed under GPL.
- MinHook 1.3.4: its license is in `MinHook.txt`.
- The RenderDoc application API header: its MIT notice is included. RenderDoc itself is not bundled.
- egui and the Rust dependencies used to build the overlay: see `RUST-DEPENDENCIES.md` and `rust/`.

NVIDIA DLSS is provided by NVIDIA Corporation. ReScaleFrame is an independent mod and is not
an NVIDIA or Bandai Namco product. Preserve the accompanying notices when redistributing files.

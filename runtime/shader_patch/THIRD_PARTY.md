# Shader patch service dependencies

Pack metadata and rules are read with serde_json (MIT or Apache-2.0), a Cargo dependency of the
Runtime.

3Dmigoto (GPLv3, 8f329bd94fecc9bbcb9211ffd42a95dd7fe6b43e) and
Zstandard (BSD option, f8745da6ff1ad1e7bab384bd1f9d742439278e99) were downloaded
under vendor/shader-patch for evaluation and retain their upstream licences.
Neither is linked or copied into the runtime. The synthetic DXBC checksum uses
a first-party Rust MD5 compression implementation with DXBC's documented padding;
3Dmigoto Assembler.cpp was an algorithm reference, not imported source.

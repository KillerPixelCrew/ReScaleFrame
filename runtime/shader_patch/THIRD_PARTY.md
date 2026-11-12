# Shader patch service dependencies

yyjson, MIT, revision ebb8d7e5aaa51bea4edefb39ee73a235066b0023.
The exact source/header/licence hashes are in yyjson.lock.json. Sources and original
licence are staged under vendor/shader-patch/yyjson; packages include its MIT notice.

3Dmigoto (GPLv3, 8f329bd94fecc9bbcb9211ffd42a95dd7fe6b43e) and
Zstandard (BSD option, f8745da6ff1ad1e7bab384bd1f9d742439278e99) were downloaded
under vendor/shader-patch for evaluation and retain their upstream licences.
Neither is linked or copied into the runtime. The synthetic DXBC checksum uses
a first-party MD5 compression implementation with DXBC's documented padding;
3Dmigoto Assembler.cpp was an algorithm reference, not imported source.

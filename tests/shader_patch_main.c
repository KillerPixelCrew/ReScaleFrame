// SPDX-License-Identifier: MIT
// Keep CTest and the corpus CLI on their existing CMake targets.
#if defined(RSF_SHADER_PATCH_DEVICE_FIXTURE)
int rsf_shader_patch_device_main(void);
int main(void) { return rsf_shader_patch_device_main(); }
#else
int rsf_shader_patch_corpus_main(void);
int main(void) { return rsf_shader_patch_corpus_main(); }
#endif

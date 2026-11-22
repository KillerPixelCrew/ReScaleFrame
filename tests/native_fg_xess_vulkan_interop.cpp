// SPDX-License-Identifier: MIT
// Only the fixture entry point uses the native CRT.
extern "C" int rsf_fixture_xess_interop(int argc, wchar_t** argv);

int wmain(int argc, wchar_t** argv) {
    return rsf_fixture_xess_interop(argc, argv);
}

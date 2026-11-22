// SPDX-License-Identifier: MIT
#include <cstdio>

extern "C" int rsf_fg_fixture_main(int argc, char** argv);

int main(int argc, char** argv) {
    std::setvbuf(stdout, nullptr, _IONBF, 0);
    return rsf_fg_fixture_main(argc, argv);
}

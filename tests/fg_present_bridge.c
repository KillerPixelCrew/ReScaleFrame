/* SPDX-License-Identifier: MIT */
#include <stdio.h>

int rsf_fg_fixture_main(int argc, char** argv);

int main(int argc, char** argv) {
    setvbuf(stdout, NULL, _IONBF, 0);
    return rsf_fg_fixture_main(argc, argv);
}

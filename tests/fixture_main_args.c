/* SPDX-License-Identifier: MIT */
/* Native entry for a Rust fixture that selects its case from the command line. */
#include <stdio.h>

extern int RSF_FIXTURE_ENTRY(int argc, char** argv);

int main(int argc, char** argv)
{
    setvbuf(stdout, NULL, _IONBF, 0);
    return RSF_FIXTURE_ENTRY(argc, argv);
}
/* SPDX-License-Identifier: MIT */
#include <stdio.h>

extern int RSF_FG_DEVICE_FIXTURE(int argc, char** argv);

int main(int argc, char** argv)
{
    setvbuf(stdout, NULL, _IONBF, 0);
    return RSF_FG_DEVICE_FIXTURE(argc, argv);
}

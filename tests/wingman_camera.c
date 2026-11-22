/* SPDX-License-Identifier: MIT */
#include <stdbool.h>
#include <stdio.h>

bool rsf_wingman_camera_fixture(void);

int main(void)
{
    if (!rsf_wingman_camera_fixture()) return 1;
    puts("PASS: Wingman projection, reprojection, frame identity, jitter and guarded refusals.");
    return 0;
}

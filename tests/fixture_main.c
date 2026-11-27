/* SPDX-License-Identifier: MIT */
/* Native entry for a Rust fixture: CMake defines RSF_FIXTURE_ENTRY as the exported fixture function. */
extern int RSF_FIXTURE_ENTRY(void);

int main(void)
{
    return RSF_FIXTURE_ENTRY();
}
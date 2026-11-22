/* SPDX-License-Identifier: MIT */
#include <rescaleframe/fg_choice.h>

/* The production FG preference is still an inline C SDK helper. */
extern int rsf_test_fg_choice(uint32_t (*default_backend)(uint32_t, uint32_t));

int main(void)
{
    return rsf_test_fg_choice(rsf_fg_default_backend);
}

// SPDX-License-Identifier: MIT
/* CTest and the deployment helper expect native executable entry points. */
extern int RSF_OVERLAY_FIXTURE(void);

int main(void)
{
    return RSF_OVERLAY_FIXTURE();
}

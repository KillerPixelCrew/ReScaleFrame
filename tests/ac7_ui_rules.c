/* SPDX-License-Identifier: MIT */
/* Native ABI smoke test and optional captured HUD shader probe. Synthetic classifier cases
   run in games/ac7/src/ui/tests.rs. */
#include <rescaleframe/ac7_ui_rules.h>

#include <stdio.h>

int main(int argc, char** argv)
{
    if (rsf_ac7_ui_classify(NULL, NULL) != RSF_AC7_DRAW_SCENE ||
        rsf_ac7_ui_classify_layout(NULL, 0) != RSF_AC7_LAYOUT_OTHER) return 1;
    if (argc != 2) return 0;

    /* The captured shader is supplied locally. */
    unsigned char bytecode[1104];
    FILE* file = fopen(argv[1], "rb");
    if (!file) return 1;
    int loaded = fseek(file, 0, SEEK_END) == 0 &&
        ftell(file) == (long)sizeof(bytecode) &&
        fseek(file, 0, SEEK_SET) == 0 &&
        fread(bytecode, 1, sizeof(bytecode), file) == sizeof(bytecode);
    fclose(file);
    if (!loaded || !rsf_ac7_ui_is_hud_producer(bytecode, (uint32_t)sizeof(bytecode))) return 1;
    bytecode[sizeof(bytecode) - 1] ^= 1;
    return rsf_ac7_ui_is_hud_producer(bytecode, (uint32_t)sizeof(bytecode)) ? 1 : 0;
}

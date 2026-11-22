/* SPDX-License-Identifier: MIT */
/* Native ABI smoke test and optional captured HUD shader probe. Synthetic classifier cases
   run in games/ac7/src/ui/tests.rs. */
#include <rescaleframe/ac7_ui_rules.h>

#include <fstream>
#include <vector>

int main(int argc, char** argv)
{
    if (rsf_ac7_ui_classify(nullptr, nullptr) != RSF_AC7_DRAW_SCENE ||
        rsf_ac7_ui_classify_layout(nullptr, 0) != RSF_AC7_LAYOUT_OTHER) return 1;
    if (argc != 2) return 0;

    // The captured shader is supplied locally.
    std::ifstream file(argv[1], std::ios::binary | std::ios::ate);
    constexpr uint32_t bytes = 1104;
    if (!file || file.tellg() != std::streamoff{bytes}) return 1;
    std::vector<char> bytecode(bytes);
    file.seekg(0);
    if (!file.read(bytecode.data(), bytes) ||
        !rsf_ac7_ui_is_hud_producer(bytecode.data(), bytes)) return 1;
    bytecode.back() ^= 1;
    return rsf_ac7_ui_is_hud_producer(bytecode.data(), bytes) ? 1 : 0;
}

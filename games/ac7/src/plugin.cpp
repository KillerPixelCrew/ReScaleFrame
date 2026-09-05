#include <rescaleframe/game_api.h>
#include <rescaleframe/version.h>

#include <cstddef>

namespace {
constexpr char known_sha256[] =
    "c7da97f5f8a807d4f1264adbb074146fcffe9bdc2ffa98791b822cd28e558f4f";

char ascii_lower(char value) noexcept
{
    return value >= 'A' && value <= 'Z' ? static_cast<char>(value + ('a' - 'A')) : value;
}

bool equal_ascii(const char* value, const char* expected) noexcept
{
    if (!value || !expected) {
        return false;
    }
    for (std::size_t i = 0;; ++i) {
        if (ascii_lower(value[i]) != ascii_lower(expected[i])) {
            return false;
        }
        if (expected[i] == '\0') {
            return true;
        }
    }
}

rsf_detection detect(const rsf_game_probe* probe) noexcept
{
    if (!probe || probe->struct_size < sizeof(rsf_game_probe) || probe->pe_machine != 0x8664) {
        return RSF_GAME_UNKNOWN;
    }
    return equal_ascii(probe->executable_name_utf8, "Ace7Game.exe") &&
                   equal_ascii(probe->sha256_hex, known_sha256)
               ? RSF_GAME_RECOGNIZED
               : RSF_GAME_UNKNOWN;
}
}

extern "C" __declspec(dllexport) rsf_result rsf_get_game_plugin_api(
    uint32_t requested_abi, rsf_game_plugin_api* api) noexcept
{
    if (!api || api->struct_size < sizeof(rsf_game_plugin_api)) {
        return RSF_ERROR_INVALID_ARGUMENT;
    }
    if (requested_abi != RSF_GAME_ABI_VERSION) {
        return RSF_ERROR_ABI_MISMATCH;
    }
    *api = {
        sizeof(rsf_game_plugin_api), RSF_GAME_ABI_VERSION,
        {"ac7", "Ace Combat 7: Skies Unknown", RSF_VERSION_STRING, 0,
         "Executable recognized only. Renderer hooks are not implemented."},
        detect};
    return RSF_OK;
}

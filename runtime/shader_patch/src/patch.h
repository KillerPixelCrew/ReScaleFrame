// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/shader_patch.h>
#include <yyjson.h>
#include <map>
#include <span>
#include <string>
#include <string_view>
#include <vector>
namespace rsf::patch {
using Files = std::map<std::string,std::string>;
struct Error { int32_t code; std::string reason; };
void check(bool, int32_t, const char*);
std::string sha256(std::string_view);
std::string_view bytes(rsf_patch_bytes);
std::string text(yyjson_val*, const char* key);
struct Pack {
    Files payload;
    std::string hash;
    std::string id;
    yyjson_doc* rules{};
    Pack() = default;
    Pack(const Pack&) = delete;
    ~Pack();
    void load(std::string_view);
    Files transform(const Files&) const;
};
}

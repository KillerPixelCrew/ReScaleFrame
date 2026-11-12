// SPDX-License-Identifier: GPL-3.0-only
#include "shader_loader.h"
#include "motion_profile.h"
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>
#include <bcrypt.h>
#include <algorithm>
#include <cstring>
#include <fstream>
#include <sstream>

namespace rsf::anomaly::motion {
const char* family_name(Family family) noexcept
{
    static constexpr const char* names[] = {"none", "core_static", "core_model", "core_pixel", "foliage_tree", "foliage_detail"};
    const auto i = static_cast<std::size_t>(family);
    return i < std::size(names) ? names[i] : "unknown";
}
bool family_is_vertex(Family family) noexcept
{
    return family == Family::CoreStatic || family == Family::CoreModel || family == Family::FoliageTree ||
        family == Family::FoliageDetail;
}
const char* compatibility_name(Compatibility value) noexcept
{
    static constexpr const char* names[] = {"ok", "version", "missing_binding", "extra_binding", "buffer", "inputs", "outputs"};
    const auto i = static_cast<std::size_t>(value);
    return i < std::size(names) ? names[i] : "unknown";
}
const char* outcome_name(Outcome value) noexcept
{
    static constexpr const char* names[] = {"not_allowlisted", "replaced", "replaced_from_cache", "unknown_caller",
        "unreadable", "original_compile_failed", "original_mismatch", "motion_compile_failed", "reflection_mismatch",
        "create_failed"};
    const auto i = static_cast<std::size_t>(value);
    return i < std::size(names) ? names[i] : "unknown";
}

namespace {
bool hex64(std::string_view text) noexcept
{
    return text.size() == 64 && std::all_of(text.begin(), text.end(), [](char c) {
        return (c >= '0' && c <= '9') || (c >= 'a' && c <= 'f');
    });
}
std::optional<Family> parse_family(std::string_view text) noexcept
{
    for (std::size_t i = 1; i < static_cast<std::size_t>(Family::Count); ++i)
        if (text == family_name(static_cast<Family>(i))) return static_cast<Family>(i);
    return std::nullopt;
}
bool safe_name(std::string_view name) noexcept
{
    if (name.empty() || name.size() > 200 || name.find("..") != std::string_view::npos) return false;
    return std::all_of(name.begin(), name.end(), [](char c) {
        return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9') || c == '_' || c == '-' || c == '.';
    });
}
std::optional<std::vector<std::byte>> read_file(const std::filesystem::path& path)
{
    std::ifstream file(path, std::ios::binary);
    if (!file) return std::nullopt;
    std::vector<char> data((std::istreambuf_iterator<char>(file)), std::istreambuf_iterator<char>());
    std::vector<std::byte> result(data.size());
    if (!data.empty()) std::memcpy(result.data(), data.data(), data.size());
    return result;
}
}

std::optional<Manifest> Manifest::parse(std::string_view text, std::string& error)
{
    Manifest result;
    std::istringstream input{std::string(text)};
    std::string line;
    std::size_t number = 0;
    bool header = false;
    while (std::getline(input, line)) {
        ++number;
        if (!line.empty() && line.back() == '\r') line.pop_back();
        if (line.empty() || line[0] == '#') continue;
        std::istringstream words(line);
        std::string kind;
        words >> kind;
        if (!header) {
            std::string version;
            words >> version;
            if (kind != "rsf-anomaly-motion-manifest" || version != "1") { error = "bad manifest header"; return std::nullopt; }
            header = true;
            continue;
        }
        if (kind == "namespace") {
            words >> result.name_space;
            if (!hex64(result.name_space)) { error = "bad namespace at line " + std::to_string(number); return std::nullopt; }
            continue;
        }
        if (kind != "root") { error = "unknown record at line " + std::to_string(number); return std::nullopt; }
        std::string stage, family, name, original, motion, extra;
        words >> stage >> family >> name >> original >> motion;
        const auto parsed = parse_family(family);
        if ((stage != "vs" && stage != "ps") || !parsed || !safe_name(name) || !hex64(original) || !hex64(motion) ||
            (words >> extra) || family_is_vertex(*parsed) != (stage == "vs")) {
            error = "bad root record at line " + std::to_string(number);
            return std::nullopt;
        }
        ManifestEntry entry{stage[0], *parsed, name, original, motion};
        // Two roots with the same original source would be ambiguous; refuse the manifest instead of guessing.
        if (!result.by_original.emplace(original, std::move(entry)).second) {
            error = "duplicate original source at line " + std::to_string(number);
            return std::nullopt;
        }
    }
    if (!header || result.name_space.empty() || result.by_original.empty()) { error = "incomplete manifest"; return std::nullopt; }
    return result;
}

std::string sha256_hex(std::span<const std::byte> data)
{
    BCRYPT_ALG_HANDLE algorithm{};
    BCRYPT_HASH_HANDLE hash{};
    unsigned char digest[32]{};
    bool ok = BCryptOpenAlgorithmProvider(&algorithm, BCRYPT_SHA256_ALGORITHM, nullptr, 0) >= 0 &&
        BCryptCreateHash(algorithm, &hash, nullptr, 0, nullptr, 0, 0) >= 0 &&
        BCryptHashData(hash, reinterpret_cast<PUCHAR>(const_cast<std::byte*>(data.data())), static_cast<ULONG>(data.size()), 0) >= 0 &&
        BCryptFinishHash(hash, digest, sizeof(digest), 0) >= 0;
    if (hash) BCryptDestroyHash(hash);
    if (algorithm) BCryptCloseAlgorithmProvider(algorithm, 0);
    if (!ok) return {};
    static constexpr char digits[] = "0123456789abcdef";
    std::string text(64, '0');
    for (std::size_t i = 0; i < 32; ++i) { text[2 * i] = digits[digest[i] >> 4]; text[2 * i + 1] = digits[digest[i] & 15]; }
    return text;
}

FrameStatus read_request(const std::byte* module, const void* return_address, const std::byte* caller_rsp,
    const FrameReader& read, ShaderRequest& request)
{
    if (!module || !caller_rsp || !read) return FrameStatus::UnknownCaller;
    bool known = false;
    for (const auto& call : create_shader_calls)
        known |= return_address == module + call.rva + call_length;
    if (!known) return FrameStatus::UnknownCaller;
    const char* source{};
    std::uint32_t length{};
    const char* entry{};
    std::uint32_t flags{};
    if (!read(caller_rsp + frame_source_offset, &source, sizeof(source)) ||
        !read(caller_rsp + frame_length_offset, &length, sizeof(length)) ||
        !read(caller_rsp + frame_entry_offset, &entry, sizeof(entry)) ||
        !read(caller_rsp + frame_flags_offset, &flags, sizeof(flags))) return FrameStatus::Unreadable;
    if (!source || !length || length > 16u * 1024 * 1024 || !entry) return FrameStatus::BadSource;
    const auto read_string = [&](const char* text, std::string& out) {
        out.clear();
        for (std::size_t i = 0; i < 512; ++i) {
            char c{};
            if (!read(text + i, &c, 1)) return false;
            if (!c) return true;
            out.push_back(c);
        }
        return false;
    };
    request.source.resize(length);
    if (!read(source, request.source.data(), length) || !read_string(entry, request.entry)) return FrameStatus::BadSource;
    request.flags = flags;
    request.defines.clear();
    for (std::uint32_t i = 0; i < frame_defines_capacity; ++i) {
        const char* pair[2]{};
        if (!read(caller_rsp + frame_defines_offset + i * sizeof(pair), pair, sizeof(pair))) return FrameStatus::BadDefines;
        if (!pair[0]) return FrameStatus::Ok;
        Define define;
        if (!read_string(pair[0], define.name) || !pair[1] || !read_string(pair[1], define.value)) return FrameStatus::BadDefines;
        request.defines.push_back(std::move(define));
    }
    return FrameStatus::BadDefines;   // no terminator inside the 128-entry array
}

Compatibility compatible(char stage, const Reflection& stock, const Reflection& variant) noexcept
{
    if (stock.version != variant.version) return Compatibility::Version;
    for (const auto& binding : stock.bindings) {
        const auto found = std::find_if(variant.bindings.begin(), variant.bindings.end(),
            [&](const Binding& b) { return b.name == binding.name; });
        if (found == variant.bindings.end() || !(*found == binding)) return Compatibility::MissingBinding;
    }
    for (const auto& binding : variant.bindings) {
        const bool stock_has = std::any_of(stock.bindings.begin(), stock.bindings.end(),
            [&](const Binding& b) { return b.name == binding.name; });
        if (stock_has) continue;
        const bool allowed = binding.type == 0 && binding.point == 13 && binding.count == 1 &&
            (stage == 'v' ? binding.name == "RSFCorePrevious" || binding.name == "RSFFoliagePrevious"
                          : binding.name == "RSFCorePixel");
        if (!allowed) return Compatibility::ExtraBinding;
    }
    for (const auto& buffer : stock.buffers) {
        const auto found = std::find_if(variant.buffers.begin(), variant.buffers.end(),
            [&](const Buffer& b) { return b.name == buffer.name; });
        if (found == variant.buffers.end() || found->size != buffer.size) return Compatibility::Buffer;
        for (const auto& variable : buffer.variables) {
            const auto v = std::find_if(found->variables.begin(), found->variables.end(),
                [&](const Variable& x) { return x.name == variable.name; });
            if (v == found->variables.end() || !(*v == variable)) return Compatibility::Buffer;
        }
    }
    if (stage == 'v') {
        if (stock.inputs != variant.inputs) return Compatibility::Inputs;
        for (const auto& output : stock.outputs) {
            const bool kept = std::any_of(variant.outputs.begin(), variant.outputs.end(), [&](const Parameter& p) {
                return p.name == output.name && p.index == output.index && p.system_value == output.system_value &&
                    p.component == output.component && p.mask == output.mask;
            });
            if (!kept) return Compatibility::Outputs;
        }
        return Compatibility::Ok;
    }
    // Pixel: the stock outputs unchanged, plus exactly SV_Target2.
    std::vector<Parameter> extra;
    for (const auto& output : variant.outputs)
        if (std::find(stock.outputs.begin(), stock.outputs.end(), output) == stock.outputs.end()) extra.push_back(output);
    for (const auto& output : stock.outputs)
        if (std::find(variant.outputs.begin(), variant.outputs.end(), output) == variant.outputs.end()) return Compatibility::Outputs;
    if (extra.size() != 1 || extra[0].index != 2 || extra[0].reg != 2 || extra[0].mask != 0xf ||
        (extra[0].name != "SV_Target" && extra[0].name != "SV_TARGET")) return Compatibility::Outputs;
    return Compatibility::Ok;
}

bool linkable(const Reflection& vertex, const Reflection& pixel) noexcept
{
    for (const auto& input : pixel.inputs) {
        const bool found = std::any_of(vertex.outputs.begin(), vertex.outputs.end(), [&](const Parameter& p) {
            return _stricmp(p.name.c_str(), input.name.c_str()) == 0 && p.index == input.index && p.reg == input.reg &&
                (p.mask & input.mask) == input.mask;
        });
        if (!found) return false;
    }
    return true;
}

int skin_mode(const std::vector<Define>& defines) noexcept
{
    int mode = -2;
    for (const auto& define : defines) {
        if (define.name == "SKIN_NONE") mode = -1;
        for (int i = 0; i <= 4; ++i)
            if (define.name == "SKIN_" + std::to_string(i)) mode = i;
    }
    return mode;
}

ShaderLoader::ShaderLoader(Manifest manifest, std::filesystem::path deployment, std::filesystem::path cache,
    Compiler& compiler, Reflector& reflector)
    : manifest_(std::move(manifest)), deployment_(std::move(deployment)), cache_(std::move(cache)),
      compiler_(compiler), reflector_(reflector)
{
}

const ManifestEntry* ShaderLoader::lookup(std::span<const std::byte> source) const
{
    const auto found = manifest_.by_original.find(sha256_hex(source));
    return found == manifest_.by_original.end() ? nullptr : &found->second;
}

std::string ShaderLoader::cache_key(const ShaderRequest& request, const ManifestEntry& entry) const
{
    std::string text = "rsf-anomaly-motion-variant 1\n" + manifest_.name_space + "\n" + entry.original_sha256 + "\n" +
        entry.motion_sha256 + "\n" + request.target + "\n" + request.entry + "\n" + std::to_string(request.flags) + "\n" +
        sha256_hex(request.bytecode) + "\n";
    for (const auto& define : request.defines) text += define.name + "=" + define.value + "\n";
    return sha256_hex(std::as_bytes(std::span(text)));
}

Prepared ShaderLoader::prepare(const ShaderRequest& request)
{
    Prepared result;
    const auto* entry = lookup(request.source);
    if (!entry || request.target.empty() || request.target[0] != entry->stage) return result;
    result.entry = entry;
    result.skin = skin_mode(request.defines);
    Reflection stock;
    if (!reflector_.reflect(request.bytecode, stock)) {
        result.outcome = Outcome::ReflectionMismatch;
        result.detail = "stock bytecode does not reflect";
        return result;
    }
    const auto check = [&](std::vector<std::byte> variant, Outcome success) {
        Reflection reflection;
        if (!reflector_.reflect(variant, reflection)) {
            result.outcome = Outcome::ReflectionMismatch;
            result.detail = "variant does not reflect";
            return false;
        }
        result.compatibility = compatible(entry->stage, stock, reflection);
        if (result.compatibility != Compatibility::Ok) {
            result.outcome = Outcome::ReflectionMismatch;
            result.detail = compatibility_name(result.compatibility);
            return false;
        }
        result.variant = std::move(variant);
        result.reflection = std::move(reflection);
        result.outcome = success;
        return true;
    };
    const auto key = cache_key(request, *entry);
    const auto cached_path = cache_.empty() ? std::filesystem::path{} : cache_ / (key + ".dxbc");
    if (!cached_path.empty()) {
        std::error_code ec;
        if (std::filesystem::is_regular_file(cached_path, ec)) {
            if (auto bytes = read_file(cached_path); bytes && !bytes->empty() && check(std::move(*bytes), Outcome::ReplacedFromCache))
                return result;
            result = Prepared{Outcome::NotAllowlisted, entry, result.skin};   // a bad cache entry falls back to compiling
        }
    }
    std::vector<std::byte> original;
    std::string error;
    if (!compiler_.compile(deployment_ / "original", request.source, request, original, error)) {
        result.outcome = Outcome::OriginalCompileFailed;
        result.detail = error;
        return result;
    }
    if (original.size() != request.bytecode.size() ||
        !std::equal(original.begin(), original.end(), request.bytecode.begin())) {
        result.outcome = Outcome::OriginalMismatch;
        result.detail = "deployed original tree does not reproduce the engine bytecode";
        return result;
    }
    const auto motion_root = read_file(deployment_ / "motion" / "r3" / entry->name);
    if (!motion_root || sha256_hex(*motion_root) != entry->motion_sha256) {
        result.outcome = Outcome::MotionCompileFailed;
        result.detail = "motion root missing or not the manifest's";
        return result;
    }
    std::vector<std::byte> variant;
    if (!compiler_.compile(deployment_ / "motion", *motion_root, request, variant, error)) {
        result.outcome = Outcome::MotionCompileFailed;
        result.detail = error;
        return result;
    }
    if (!check(std::move(variant), Outcome::Replaced)) return result;
    if (!cached_path.empty()) {
        std::error_code ec;
        std::filesystem::create_directories(cache_, ec);
        const auto temporary = cached_path.string() + ".tmp";
        {
            std::ofstream file(temporary, std::ios::binary | std::ios::trunc);
            if (file) file.write(reinterpret_cast<const char*>(result.variant.data()), static_cast<std::streamsize>(result.variant.size()));
        }
        std::filesystem::rename(temporary, cached_path, ec);
        if (ec) std::filesystem::remove(temporary, ec);
    }
    return result;
}
}

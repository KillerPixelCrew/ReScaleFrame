// SPDX-License-Identifier: MIT
#include "shader_loader.h"
#include "shader_loader_platform.h"
#include <utility>

namespace rsf::anomaly::motion {
namespace {
using Bytes = rsf_anomaly_shader_bytes;

Bytes bytes(std::string_view text) noexcept
{
    return {reinterpret_cast<const uint8_t*>(text.data()), text.size()};
}
Bytes bytes(std::span<const std::byte> data) noexcept
{
    return {reinterpret_cast<const uint8_t*>(data.data()), data.size()};
}
std::string_view text(Bytes view) noexcept
{
    return view.len ? std::string_view(reinterpret_cast<const char*>(view.data), view.len) : std::string_view{};
}
std::span<const std::byte> bytecode(Bytes view) noexcept
{
    return {reinterpret_cast<const std::byte*>(view.data), view.len};
}
rsf_anomaly_shader_wide wide(const std::filesystem::path& path) noexcept
{
    static_assert(sizeof(wchar_t) == sizeof(uint16_t));
    return {reinterpret_cast<const uint16_t*>(path.native().data()), path.native().size()};
}
std::vector<rsf_anomaly_shader_define> define_views(const std::vector<Define>& defines)
{
    std::vector<rsf_anomaly_shader_define> views;
    views.reserve(defines.size());
    for (const auto& d : defines) views.push_back({bytes(d.name), bytes(d.value)});
    return views;
}
struct RequestView {
    std::vector<rsf_anomaly_shader_define> defines;
    rsf_anomaly_shader_request value;

    explicit RequestView(const ShaderRequest& r)
        : defines(define_views(r.defines)), value{bytes(r.target), bytes(r.entry), r.flags,
            {defines.data(), defines.size()}, bytes(r.source), bytes(r.bytecode)} {}
};
rsf_anomaly_shader_identity identity(const Manifest& manifest, const ManifestEntry& entry) noexcept
{
    return {bytes(manifest.name_space), bytes(entry.original_sha256), bytes(entry.motion_sha256), bytes(entry.name),
        static_cast<uint8_t>(entry.stage)};
}

struct ManifestBuilder {
    Manifest value;
    std::string& error;

    rsf_anomaly_shader_manifest_sink sink() noexcept
    {
        return {this, add, finish};
    }
    static bool add(void* user, const rsf_anomaly_shader_root* root, uint8_t family) noexcept try
    {
        auto& manifest = static_cast<ManifestBuilder*>(user)->value;
        ManifestEntry entry{static_cast<char>(root->stage), static_cast<Family>(family), std::string(text(root->name)),
            std::string(text(root->original)), std::string(text(root->motion))};
        const auto hash = entry.original_sha256;
        return manifest.by_original.emplace(hash, std::move(entry)).second;
    } catch (...) { return false; }

    static bool finish(void* user, Bytes message, bool accepted) noexcept try
    {
        auto& self = *static_cast<ManifestBuilder*>(user);
        (accepted ? self.value.name_space : self.error) = text(message);
        return true;
    } catch (...) { return false; }
};

template<class T, class U, class Project>
void project(const std::vector<T>& input, std::vector<U>& output, Project row)
{
    output.clear();
    output.reserve(input.size());
    for (const auto& item : input) output.push_back(row(item));
}

struct ReflectionView {
    std::vector<rsf_anomaly_shader_parameter> inputs, outputs, patch;
    std::vector<rsf_anomaly_shader_binding> bindings;
    std::vector<rsf_anomaly_shader_buffer> buffers;
    std::vector<rsf_anomaly_shader_variable> variables;

    rsf_anomaly_shader_reflection borrow(const Reflection& r, bool signatures_only = false)
    {
        const auto param = [](const Parameter& p) {
            return rsf_anomaly_shader_parameter{bytes(p.name), p.index, p.reg, p.system_value, p.component, p.mask};
        };
        project(r.inputs, inputs, param);
        project(r.outputs, outputs, param);
        project(r.patch, patch, param);
        if (!signatures_only) {
            project(r.bindings, bindings, [](const Binding& b) {
                return rsf_anomaly_shader_binding{bytes(b.name), b.type, b.point, b.count, b.return_type, b.dimension};
            });
            std::size_t total = 0;
            for (const auto& b : r.buffers) total += b.variables.size();
            variables.clear();
            variables.reserve(total);
            project(r.buffers, buffers, [&](const Buffer& b) {
                const auto start = variables.size();
                for (const auto& v : b.variables) variables.push_back({bytes(v.name), v.offset, v.size});
                return rsf_anomaly_shader_buffer{bytes(b.name), b.size,
                    {b.variables.empty() ? nullptr : variables.data() + start, b.variables.size()}};
            });
        }
        return {r.version, {inputs.data(), inputs.size()}, {outputs.data(), outputs.size()}, {patch.data(), patch.size()},
            {bindings.data(), bindings.size()}, {buffers.data(), buffers.size()}};
    }
};

struct PreparationBridge {
    Prepared result;
    Reflector& reflector;
    ReflectionView stock, variant;

    PreparationBridge(const ManifestEntry& entry, Reflector& r) : reflector(r) { result.entry = &entry; }
    static bool reflect(void* user, bool is_stock, Bytes code, rsf_anomaly_shader_reflection* out) noexcept try
    {
        auto& self = *static_cast<PreparationBridge*>(user);
        auto& reflection = is_stock ? self.result.stock : self.result.reflection;
        reflection = {};
        if (!self.reflector.reflect(bytecode(code), reflection)) {
            reflection = {};
            return false;
        }
        *out = (is_stock ? self.stock : self.variant).borrow(reflection);
        return true;
    } catch (...) { return false; }

    static void complete(void* user, uint8_t outcome, uint8_t compatibility, int32_t skin,
        Bytes detail, Bytes variant) noexcept try
    {
        auto& result = static_cast<PreparationBridge*>(user)->result;
        result.detail = text(detail);
        if (variant.len) {
            const auto code = bytecode(variant);
            result.variant.assign(code.begin(), code.end());
        } else {
            result.reflection = {};
        }
        result.compatibility = static_cast<Compatibility>(compatibility);
        result.skin = skin;
        result.outcome = static_cast<Outcome>(outcome);
    } catch (...) {}

    rsf_anomaly_shader_preparation sink() noexcept { return {this, reflect, complete}; }
};

struct TreePreparation : PreparationBridge {
    const ShaderRequest& request;
    const std::filesystem::path& deployment;
    Compiler& compiler;
    std::vector<std::byte> code;
    std::string error;

    TreePreparation(const ManifestEntry& entry, Reflector& reflector, const ShaderRequest& r,
        const std::filesystem::path& directory, Compiler& c)
        : PreparationBridge(entry, reflector), request(r), deployment(directory), compiler(c) {}

    static bool compile(void* user, bool motion, Bytes source, Bytes* out) noexcept try
    {
        auto& self = *static_cast<TreePreparation*>(static_cast<PreparationBridge*>(user));
        self.error.clear();
        const bool ok = self.compiler.compile(self.deployment / (motion ? "motion" : "original"),
            bytecode(source), self.request, self.code, self.error);
        *out = ok ? bytes(self.code) : bytes(self.error);
        return ok;
    } catch (...) {
        *out = bytes("compiler callback failed");
        return false;
    }
};

bool read_frame(const void* user, uintptr_t address, uint8_t* out, std::size_t size) noexcept try
{
    return (*static_cast<const FrameReader*>(user))(reinterpret_cast<const void*>(address), out, size);
} catch (...) { return false; }

bool publish_frame(void* user, const rsf_anomaly_shader_decoded_frame* decoded) noexcept try
{
    auto& out = *static_cast<ShaderRequest*>(user);
    const auto source = bytecode(decoded->source);
    out.source.assign(source.begin(), source.end());
    out.entry = text(decoded->entry);
    out.flags = decoded->flags;
    out.defines.clear();
    out.defines.reserve(decoded->defines.len);
    for (std::size_t i = 0; i < decoded->defines.len; ++i) {
        const auto& define = decoded->defines.data[i];
        out.defines.push_back({std::string(text(define.name)), std::string(text(define.value))});
    }
    return true;
} catch (...) { return false; }
}

const char* family_name(Family family) noexcept { return rsf_anomaly_shader_family_name(static_cast<uint8_t>(family)); }
char family_stage(Family family) noexcept { return static_cast<char>(rsf_anomaly_shader_family_stage(static_cast<uint8_t>(family))); }
bool family_is_vertex(Family family) noexcept { return family_stage(family) == 'v'; }
const char* compatibility_name(Compatibility value) noexcept
{
    return rsf_anomaly_shader_compatibility_name(static_cast<uint8_t>(value));
}
const char* outcome_name(Outcome value) noexcept
{
    static constexpr const char* names[] = {"not_allowlisted", "replaced", "replaced_from_cache", "unknown_caller",
        "unreadable", "original_compile_failed", "original_mismatch", "motion_compile_failed", "reflection_mismatch",
        "create_failed"};
    const auto i = static_cast<std::size_t>(value);
    return i < std::size(names) ? names[i] : "unknown";
}

std::optional<Manifest> Manifest::parse(std::string_view input, std::string& error)
{
    ManifestBuilder builder{{}, error};
    const auto sink = builder.sink();
    if (!rsf_anomaly_shader_manifest(bytes(input), &sink)) return std::nullopt;
    return std::move(builder.value);
}
bool admit_runtime_manifest(Manifest& manifest, std::string_view name_space, uint32_t count,
    const void* session, rsf_anomaly_shader_read_root read, std::string& error)
{
    ManifestBuilder builder{{}, error};
    const auto sink = builder.sink();
    if (!rsf_anomaly_shader_pack_manifest(bytes(name_space), count, session, read, &sink)) return false;
    manifest = std::move(builder.value);
    return true;
}
std::string sha256_hex(std::span<const std::byte> data)
{
    uint8_t digest[64];
    return rsf_anomaly_shader_sha256(bytes(data), &digest) ? std::string(text({digest, sizeof(digest)})) : std::string{};
}

FrameStatus read_request(const MotionProfile& profile, const std::byte* module, const void* return_address,
    const std::byte* caller_rsp, const FrameReader& reader, ShaderRequest& request)
{
    if (!reader) return FrameStatus::UnknownCaller;
    const auto& frame = profile.frame;
    const rsf_anomaly_shader_frame_layout layout{{profile.create_shader_calls[0].rva, profile.create_shader_calls[1].rva},
        call_length, frame.source_offset, frame.length_offset, frame.entry_offset, frame.flags_offset, frame.defines_offset,
        frame_defines_capacity};
    return static_cast<FrameStatus>(rsf_anomaly_shader_frame(&layout, reinterpret_cast<uintptr_t>(module),
        reinterpret_cast<uintptr_t>(return_address), reinterpret_cast<uintptr_t>(caller_rsp),
        &reader, read_frame, &request, publish_frame));
}

Compatibility compatible(char stage, const Reflection& stock, const Reflection& variant) noexcept try
{
    ReflectionView a, b;
    const auto old = a.borrow(stock), next = b.borrow(variant);
    return static_cast<Compatibility>(rsf_anomaly_shader_compatible(static_cast<uint8_t>(stage), &old, &next));
} catch (...) { return Compatibility::Version; }

bool linkable(const Reflection& vertex, const Reflection& pixel) noexcept try
{
    ReflectionView a, b;
    const auto vs = a.borrow(vertex, true), ps = b.borrow(pixel, true);
    return rsf_anomaly_shader_linkable(&vs, &ps);
} catch (...) { return false; }

bool tess_chain_linkable(const Reflection& vertex, const Reflection& hull, const Reflection& domain, const Reflection& pixel) noexcept try
{
    ReflectionView a, b, c, d;
    const auto vs = a.borrow(vertex, true), hs = b.borrow(hull, true), ds = c.borrow(domain, true), ps = d.borrow(pixel, true);
    return rsf_anomaly_shader_tess_linkable(&vs, &hs, &ds, &ps);
} catch (...) { return false; }

int skin_mode(const std::vector<Define>& defines) noexcept try
{
    const auto views = define_views(defines);
    return rsf_anomaly_shader_skin_mode({views.data(), views.size()});
} catch (...) { return -2; }

ShaderLoader::ShaderLoader(Manifest manifest, std::filesystem::path deployment, std::filesystem::path cache,
    Compiler& compiler, Reflector& reflector)
    : manifest_(std::move(manifest)), deployment_(std::move(deployment)), cache_(std::move(cache)),
      compiler_(compiler), reflector_(reflector) {}

ShaderLoader::ShaderLoader(Manifest manifest, Compiler& compiler, Reflector& reflector, Prepare prepare)
    : manifest_(std::move(manifest)), compiler_(compiler), reflector_(reflector), runtime_prepare_(std::move(prepare)) {}

const ManifestEntry* ShaderLoader::lookup(std::span<const std::byte> source) const
{
    const auto found = manifest_.by_original.find(sha256_hex(source));
    return found == manifest_.by_original.end() ? nullptr : &found->second;
}
std::string ShaderLoader::cache_key(const ShaderRequest& request, const ManifestEntry& entry) const
{
    const RequestView input(request);
    const auto id = identity(manifest_, entry);
    uint8_t digest[64];
    return rsf_anomaly_shader_cache_key(&input.value, &id, &digest)
        ? std::string(text({digest, sizeof(digest)})) : std::string{};
}
Prepared ShaderLoader::prepare(const ShaderRequest& request)
{
    const auto* entry = lookup(request.source);
    if (!entry || !rsf_anomaly_shader_stage_matches(bytes(request.target), static_cast<uint8_t>(entry->stage))) return {};
    if (runtime_prepare_) return runtime_prepare_(request, *entry);
    const RequestView input(request);
    const auto id = identity(manifest_, *entry);
    TreePreparation bridge(*entry, reflector_, request, deployment_, compiler_);
    const auto host = bridge.sink();
    rsf_anomaly_shader_prepare_tree(&input.value, &id, wide(deployment_), wide(cache_), &host, TreePreparation::compile);
    return std::move(bridge.result);
}
Prepared finish_runtime_shader(const ShaderRequest& request, const ManifestEntry& entry, Reflector& reflector,
    int32_t status, bool cache_hit, std::span<const std::byte> code, std::string_view detail)
{
    const RequestView input(request);
    PreparationBridge bridge(entry, reflector);
    const auto host = bridge.sink();
    rsf_anomaly_shader_finish_runtime(&input.value, static_cast<uint8_t>(entry.stage), status, cache_hit, bytes(code),
        bytes(detail), &host);
    return std::move(bridge.result);
}
}

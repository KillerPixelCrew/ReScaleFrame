// SPDX-License-Identifier: MIT
#include "shader_loader.h"
#include "shader_loader_platform.h"
#include <windows.h>
#include <new>
#include <utility>

namespace rsf::anomaly::motion {
using OpenFile = HRESULT (*)(const void*, const char*, const char*, HANDLE*);
extern "C" {
rsf_anomaly_shader_snapshot* rsf_anomaly_shader_d3d_reflect(const rsf_anomaly_shader_d3d*, rsf_anomaly_shader_bytes) noexcept;
const rsf_anomaly_shader_reflection* rsf_anomaly_shader_snapshot_view(const rsf_anomaly_shader_snapshot*) noexcept;
bool rsf_anomaly_shader_snapshot_keeps_buffer_sizes(const rsf_anomaly_shader_snapshot*, const rsf_anomaly_shader_snapshot*) noexcept;
bool rsf_anomaly_shader_d3d_compile(const rsf_anomaly_shader_d3d*, const void*, OpenFile,
    const rsf_anomaly_shader_request*, void*, bool (*)(void*, bool, rsf_anomaly_shader_bytes)) noexcept;
void rsf_anomaly_shader_d3d_prepare(const rsf_anomaly_shader_d3d*, const rsf_anomaly_shader_admitted*,
    const rsf_anomaly_shader_request*, rsf_anomaly_shader_wide, const void*, const void*, OpenFile, void*,
    bool (*)(void*, bool, rsf_anomaly_shader_bytes),
    void (*)(void*, const rsf_anomaly_shader_entry*, uint8_t, uint8_t, int32_t, rsf_anomaly_shader_bytes,
        rsf_anomaly_shader_bytes, rsf_anomaly_shader_snapshot*, rsf_anomaly_shader_snapshot*)) noexcept;
}
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
// Receives the single refusal reason of a manifest candidate.
struct Refusal {
    std::string& error;

    rsf_anomaly_shader_refusal sink() noexcept { return {this, report}; }
    static void report(void* user, Bytes message) noexcept try
    {
        static_cast<Refusal*>(user)->error = text(message);
    } catch (...) {}
};

HRESULT open_file(const void* tree, const char* prefix, const char* name, HANDLE* out) noexcept try {
    // Keep the STL's narrow filename conversion and Windows root/drive/UNC append rules.
    // make_preferred also converts slashes in include names for extended-length trees.
    auto path = *static_cast<const std::filesystem::path*>(tree);
    if (prefix) path /= prefix;
    path /= std::filesystem::path(name).make_preferred();
    // Match the previous binary stream's read access and sharing on this native path.
    const HANDLE file = CreateFileW(path.c_str(), GENERIC_READ, FILE_SHARE_READ | FILE_SHARE_WRITE,
        nullptr, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, nullptr);
    if (file == INVALID_HANDLE_VALUE) return S_FALSE;
    *out = file;
    return S_OK;
} catch (const std::bad_alloc&) {
    return E_OUTOFMEMORY;
} catch (...) {
    return E_FAIL;
}

}

const char* outcome_name(Outcome value) noexcept
{
    return rsf_anomaly_shader_outcome_name(static_cast<uint8_t>(value));
}

Manifest::Manifest(rsf_anomaly_shader_admitted* owned) noexcept
{
    if (!owned) return;
    try {
        owner_.reset(owned, rsf_anomaly_shader_admitted_free);
    } catch (...) {
        // shared_ptr frees the object itself when its control block cannot be allocated.
        return;
    }
    rsf_anomaly_shader_manifest_view view{};
    rsf_anomaly_shader_admitted_view(owned, &view);
    name_space_ = text(view.name_space);
    if (view.count) entries_ = {&manifest_entry(view.entries[0]), view.count};
}
const ManifestEntry* Manifest::find(std::string_view original_sha256) const noexcept
{
    const auto* row = owner_ ? rsf_anomaly_shader_admitted_find(owner_.get(), bytes(original_sha256)) : nullptr;
    return row ? &manifest_entry(*row) : nullptr;
}
std::optional<Manifest> Manifest::parse(std::string_view input, std::string& error)
{
    Refusal refusal{error};
    const auto sink = refusal.sink();
    auto* admitted = rsf_anomaly_shader_manifest(bytes(input), &sink);
    if (!admitted) return std::nullopt;
    Manifest manifest(admitted);
    if (!manifest.admitted()) {
        error = "manifest storage failed";
        return std::nullopt;
    }
    return manifest;
}
const rsf_anomaly_shader_reflection* reflection_view(const Reflection& reflection) noexcept
{
    return rsf_anomaly_shader_snapshot_view(reflection.get());
}

bool keeps_buffer_sizes(const Reflection& stock, const Reflection& variant) noexcept
{
    return rsf_anomaly_shader_snapshot_keeps_buffer_sizes(stock.get(), variant.get());
}

bool linkable(const Reflection& vertex, const Reflection& pixel) noexcept
{
    const auto* vs = reflection_view(vertex), *ps = reflection_view(pixel);
    return vs && ps && rsf_anomaly_shader_linkable(vs, ps);
}

bool tess_chain_linkable(const Reflection& vertex, const Reflection& hull, const Reflection& domain, const Reflection& pixel) noexcept
{
    const auto* vs = reflection_view(vertex), *hs = reflection_view(hull), *ds = reflection_view(domain), *ps = reflection_view(pixel);
    return vs && hs && ds && ps && rsf_anomaly_shader_tess_linkable(vs, hs, ds, ps);
}

bool compile_deployed(const rsf_anomaly_shader_d3d& modules, const std::filesystem::path& tree,
    const ShaderRequest& request, std::vector<std::byte>& code, std::string& error)
{
    code.clear();
    const RequestView input(request);
    struct Output { std::vector<std::byte>& code; std::string& error; } output{code, error};
    return rsf_anomaly_shader_d3d_compile(&modules, &tree, open_file, &input.value, &output,
        [](void* user, bool ok, Bytes value) noexcept {
            try {
                auto& out = *static_cast<Output*>(user);
                if (!ok) {
                    out.error = text(value);
                    return false;
                }
                const auto data = bytecode(value);
                if (!data.empty()) out.code.assign(data.begin(), data.end());
                return true;
            } catch (...) {
                return false;
            }
        });
}

bool reflect_deployed(const rsf_anomaly_shader_d3d& modules, std::span<const std::byte> code, Reflection& out)
{
    out.reset(rsf_anomaly_shader_d3d_reflect(&modules, bytes(code)));
    return out != nullptr;
}

namespace {
struct Preparation {
    Prepared& result;
    std::vector<std::byte>* stock{};
    std::string* error{};
    bool compiled{};
};

void prepare(const rsf_anomaly_shader_d3d& modules, const Manifest& manifest, const std::filesystem::path& deployment,
    const ShaderRequest& request, Preparation& output)
{
    if (!manifest.admitted()) return;
    const auto original = deployment / "original", motion = deployment / "motion";
    const RequestView input(request);
    const auto stock_sink = [](void* user, bool ok, Bytes value) noexcept {
        auto& out = *static_cast<Preparation*>(user);
        try {
            if (!ok) {
                *out.error = text(value);
                return false;
            }
            const auto data = bytecode(value);
            if (!data.empty()) out.stock->assign(data.begin(), data.end());
            out.compiled = true;
            return true;
        } catch (...) {
            return false;
        }
    };
    rsf_anomaly_shader_d3d_prepare(&modules, manifest.admitted(), &input.value, wide(deployment), &original, &motion,
        open_file, &output, output.stock ? +stock_sink : nullptr,
        [](void* user, const rsf_anomaly_shader_entry* entry, uint8_t outcome, uint8_t compatibility, int32_t skin,
            Bytes detail, Bytes variant, rsf_anomaly_shader_snapshot* stock,
            rsf_anomaly_shader_snapshot* reflection) noexcept {
            Prepared next;
            next.stock.reset(stock);
            next.reflection.reset(reflection);
            if (variant.len && (!next.stock || !next.reflection)) return;
            try {
                next.entry = &manifest_entry(*entry);
                next.outcome = static_cast<Outcome>(outcome);
                next.compatibility = static_cast<Compatibility>(compatibility);
                next.skin = skin;
                next.detail = text(detail);
                if (variant.len) {
                    const auto code = bytecode(variant);
                    next.variant.assign(code.begin(), code.end());
                }
                static_cast<Preparation*>(user)->result = std::move(next);
            } catch (...) {}
        });
}
}

Prepared prepare_deployed(const rsf_anomaly_shader_d3d& modules, const Manifest& manifest,
    const std::filesystem::path& deployment, const ShaderRequest& request)
{
    Prepared result;
    Preparation output{result};
    prepare(modules, manifest, deployment, request, output);
    return result;
}

bool build_deployed(const rsf_anomaly_shader_d3d& modules, const Manifest& manifest, const std::filesystem::path& deployment,
    const ShaderRequest& request, std::vector<std::byte>& stock, Prepared& result, std::string& error)
{
    stock.clear();
    result = {};
    Preparation output{result, &stock, &error};
    prepare(modules, manifest, deployment, request, output);
    return output.compiled;
}
}

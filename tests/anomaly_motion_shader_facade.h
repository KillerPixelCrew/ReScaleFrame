// SPDX-License-Identifier: MIT
#pragma once
// C++ projection of the Rust-owned Anomaly shader loader for the three standalone device fixtures
// (anomaly_motion_device, anomaly_motion_far_device, anomaly_motion_tess_chain_device).
// It only converts between the fixtures' std::vector/std::string values and the C ABI in
// shader_loader_platform.h. Manifest admission, tree compilation, include lookup, stock proof, reflection and
// linkage all run in Rust (games/anomaly/src/shader_loader*). This header goes away with those fixtures.
//
// Motion shader variants (see shader_loader.rs): the engine keeps its stock shader; for an allowlisted root RSF
// builds the matching motion variant of that permutation, proves the original tree reproduces the engine's
// bytecode byte for byte, compiles the patched tree with the same inputs and requires its reflection to keep
// every stock resource and constant-buffer layout and the VS input signature.
#include "motion_profile.h"
#include "shader_loader_platform.h"
#include <cstddef>
#include <cstdint>
#include <filesystem>
#include <memory>
#include <optional>
#include <span>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

namespace rsf::anomaly::motion {
// TessHull/TessDomain: DX11\tess.hs and DX11\tess.ds of X-Ray's G-buffer tessellation pass (uber_deffer.cpp r_TessPass,
// r4_enable_tessellation): the motion hull stage carries the previous control data, the motion domain stage rebuilds the
// previous PN patch and writes the previous clip position for the motion PS.
// Stable byte values shared with shader_family.rs.
enum class Family : std::uint8_t {
    None = 0, CoreStatic = 1, CoreModel = 2, CorePixel = 3, FoliageTree = 4,
    FoliageDetail = 5, TessHull = 6, TessDomain = 7, Count = 8
};
// Borrowed manifest text, not NUL-terminated.
struct ManifestText {
    const char* data;
    std::size_t size;
    operator std::string_view() const noexcept { return {data, size}; }
};
// One admitted root (rsf_anomaly_shader_entry). Rows are owned by the Rust manifest and stay valid while a Manifest
// sharing it lives.
struct ManifestEntry {
    ManifestText name;       // root file name, relative to r3 (one subdirectory allowed: dx11/tess.hs)
    ManifestText original_sha256, motion_sha256;
    char stage;              // 'v', 'p', 'h' or 'd'
    Family family;
};
static_assert(sizeof(ManifestEntry) == sizeof(rsf_anomaly_shader_entry) && sizeof(ManifestEntry) == 56 &&
    offsetof(ManifestEntry, name) == offsetof(rsf_anomaly_shader_entry, name) &&
    offsetof(ManifestEntry, original_sha256) == offsetof(rsf_anomaly_shader_entry, original) &&
    offsetof(ManifestEntry, motion_sha256) == offsetof(rsf_anomaly_shader_entry, motion) &&
    offsetof(ManifestEntry, stage) == offsetof(rsf_anomaly_shader_entry, stage) &&
    offsetof(ManifestEntry, family) == offsetof(rsf_anomaly_shader_entry, family) &&
    sizeof(ManifestText) == sizeof(rsf_anomaly_shader_bytes) && sizeof(Family) == 1);

namespace facade {
using Bytes = rsf_anomaly_shader_bytes;

inline Bytes bytes(std::string_view text) noexcept
{
    return {reinterpret_cast<const uint8_t*>(text.data()), text.size()};
}
inline Bytes bytes(std::span<const std::byte> data) noexcept
{
    return {reinterpret_cast<const uint8_t*>(data.data()), data.size()};
}
inline std::string_view text(Bytes view) noexcept
{
    return view.len ? std::string_view(reinterpret_cast<const char*>(view.data), view.len) : std::string_view{};
}
inline std::span<const std::byte> bytecode(Bytes view) noexcept
{
    return {reinterpret_cast<const std::byte*>(view.data), view.len};
}
inline rsf_anomaly_shader_wide wide(const std::filesystem::path& path) noexcept
{
    static_assert(sizeof(wchar_t) == sizeof(uint16_t));
    return {reinterpret_cast<const uint16_t*>(path.native().data()), path.native().size()};
}
// ManifestEntry is the C++ spelling of one Rust-owned row.
inline const ManifestEntry& manifest_entry(const rsf_anomaly_shader_entry& row) noexcept
{
    return *reinterpret_cast<const ManifestEntry*>(&row);
}
}

// Line format (tools/anomaly_motion_deploy.py):
//   rsf-anomaly-motion-manifest 1
//   namespace <64 hex>
//   root <vs|ps|hs|ds> <family> <name> <original sha256> <motion sha256>
// Rust validates either form and owns the rows and their original-SHA-256 index. Copies share that immutable manifest;
// a default Manifest is empty.
class Manifest {
public:
    Manifest() noexcept = default;
    // Takes ownership of an admitted manifest.
    explicit Manifest(rsf_anomaly_shader_admitted* owned) noexcept
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
        name_space_ = facade::text(view.name_space);
        if (view.count) entries_ = {&facade::manifest_entry(view.entries[0]), view.count};
    }
    static std::optional<Manifest> parse(std::string_view input, std::string& error)
    {
        struct Refusal {
            std::string& error;
            static void report(void* user, rsf_anomaly_shader_bytes message) noexcept try
            {
                static_cast<Refusal*>(user)->error = facade::text(message);
            } catch (...) {}
        } refusal{error};
        const rsf_anomaly_shader_refusal sink{&refusal, Refusal::report};
        auto* admitted = rsf_anomaly_shader_manifest(facade::bytes(input), &sink);
        if (!admitted) return std::nullopt;
        Manifest manifest(admitted);
        if (!manifest.admitted()) {
            error = "manifest storage failed";
            return std::nullopt;
        }
        return manifest;
    }
    std::string_view name_space() const noexcept { return name_space_; }
    // Ordered by the original root source SHA-256.
    std::span<const ManifestEntry> entries() const noexcept { return entries_; }
    const ManifestEntry* find(std::string_view original_sha256) const noexcept
    {
        const auto* row = owner_ ? rsf_anomaly_shader_admitted_find(owner_.get(), facade::bytes(original_sha256)) : nullptr;
        return row ? &facade::manifest_entry(*row) : nullptr;
    }
    const rsf_anomaly_shader_admitted* admitted() const noexcept { return owner_.get(); }
private:
    std::shared_ptr<const rsf_anomaly_shader_admitted> owner_;
    std::string_view name_space_;
    std::span<const ManifestEntry> entries_;
};

struct Define {
    std::string name, value;
    bool operator==(const Define&) const = default;
};
// What create_shader and its caller frame say about one permutation.
struct ShaderRequest {
    std::string target;                 // create_shader's pTarget, already the final vs_5_0/ps_5_0
    std::string entry;
    std::uint32_t flags{};
    std::vector<Define> defines;        // engine order
    std::vector<std::byte> source;      // the engine's root source text
    std::span<const std::byte> bytecode;  // what the engine just created (borrowed)
    std::string file_name;              // engine cache/object path, diagnostics only
};
struct ReflectionDrop {
    void operator()(rsf_anomaly_shader_snapshot* snapshot) const noexcept { rsf_anomaly_shader_snapshot_drop(snapshot); }
};
using Reflection = std::unique_ptr<rsf_anomaly_shader_snapshot, ReflectionDrop>;

// D3D_SIT_CBUFFER = 0.
enum class Compatibility : std::uint8_t { Ok, Version, MissingBinding, ExtraBinding, Buffer, Inputs, Outputs, PatchConstants };
enum class Outcome : std::uint8_t {
    NotAllowlisted, Replaced, ReplacedFromCache, UnknownCaller, Unreadable, OriginalCompileFailed, OriginalMismatch,
    MotionCompileFailed, ReflectionMismatch, CreateFailed, Count
};
struct Prepared {
    Outcome outcome{Outcome::NotAllowlisted};
    const ManifestEntry* entry{};
    int skin{-2};                      // -1 SKIN_NONE, 0..4 SKIN_n, -2 none of the skin defines
    std::vector<std::byte> variant;
    Reflection reflection;             // of the variant
    Reflection stock;                  // of the engine's own bytecode (its constant-buffer indices are the engine's)
    Compatibility compatibility{};
    std::string detail;
};

inline const char* outcome_name(Outcome value) noexcept
{
    return rsf_anomaly_shader_outcome_name(static_cast<uint8_t>(value));
}
inline const rsf_anomaly_shader_reflection* reflection_view(const Reflection& reflection) noexcept
{
    return rsf_anomaly_shader_snapshot_view(reflection.get());
}
inline bool keeps_buffer_sizes(const Reflection& stock, const Reflection& variant) noexcept
{
    return rsf_anomaly_shader_snapshot_keeps_buffer_sizes(stock.get(), variant.get());
}
// Linkage of a producer stage with a consumer stage: every consumer input must be a producer output with the same name,
// index and register and a covering mask.
inline bool linkable(const Reflection& vertex, const Reflection& pixel) noexcept
{
    const auto* vs = reflection_view(vertex), *ps = reflection_view(pixel);
    return vs && ps && rsf_anomaly_shader_linkable(vs, ps);
}
// The full tessellated chain VS -> HS -> DS -> PS: each control-point interface links, and every domain patch-constant
// input is a hull patch-constant output with the same register.
inline bool tess_chain_linkable(const Reflection& vertex, const Reflection& hull, const Reflection& domain,
    const Reflection& pixel) noexcept
{
    const auto* vs = reflection_view(vertex), *hs = reflection_view(hull), *ds = reflection_view(domain),
        *ps = reflection_view(pixel);
    return vs && hs && ds && ps && rsf_anomaly_shader_tess_linkable(vs, hs, ds, ps);
}

namespace facade {
struct RequestView {
    std::vector<rsf_anomaly_shader_define> defines;
    rsf_anomaly_shader_request value;

    explicit RequestView(const ShaderRequest& r)
        : defines(define_views(r.defines)), value{bytes(r.target), bytes(r.entry), r.flags,
            {defines.data(), defines.size()}, bytes(r.source), bytes(r.bytecode)} {}
private:
    static std::vector<rsf_anomaly_shader_define> define_views(const std::vector<Define>& defines)
    {
        std::vector<rsf_anomaly_shader_define> views;
        views.reserve(defines.size());
        for (const auto& d : defines) views.push_back({bytes(d.name), bytes(d.value)});
        return views;
    }
};
struct Preparation {
    Prepared& result;
    std::vector<std::byte>* stock{};
    std::string* error{};
    bool compiled{};
};
inline void prepare(const rsf_anomaly_shader_d3d& modules, const Manifest& manifest,
    const std::filesystem::path& deployment, const ShaderRequest& request, Preparation& output)
{
    if (!manifest.admitted()) return;
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
    rsf_anomaly_shader_d3d_prepare(&modules, manifest.admitted(), &input.value, wide(deployment), &output,
        output.stock ? +stock_sink : nullptr,
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

// Device fixture projections. Rust runs the real compiler, stock proof, and reflection.
inline bool compile_deployed(const rsf_anomaly_shader_d3d& modules, const std::filesystem::path& tree,
    const ShaderRequest& request, std::vector<std::byte>& code, std::string& error)
{
    code.clear();
    const facade::RequestView input(request);
    struct Output { std::vector<std::byte>& code; std::string& error; } output{code, error};
    return rsf_anomaly_shader_d3d_compile(&modules, facade::wide(tree), &input.value, &output,
        [](void* user, bool ok, facade::Bytes value) noexcept {
            try {
                auto& out = *static_cast<Output*>(user);
                if (!ok) {
                    out.error = facade::text(value);
                    return false;
                }
                const auto data = facade::bytecode(value);
                if (!data.empty()) out.code.assign(data.begin(), data.end());
                return true;
            } catch (...) {
                return false;
            }
        });
}
inline bool reflect_deployed(const rsf_anomaly_shader_d3d& modules, std::span<const std::byte> code, Reflection& out)
{
    out.reset(rsf_anomaly_shader_d3d_reflect(&modules, facade::bytes(code)));
    return out != nullptr;
}
inline Prepared prepare_deployed(const rsf_anomaly_shader_d3d& modules, const Manifest& manifest,
    const std::filesystem::path& deployment, const ShaderRequest& request)
{
    Prepared result;
    facade::Preparation output{result};
    facade::prepare(modules, manifest, deployment, request, output);
    return result;
}
inline bool build_deployed(const rsf_anomaly_shader_d3d& modules, const Manifest& manifest,
    const std::filesystem::path& deployment, const ShaderRequest& request, std::vector<std::byte>& stock,
    Prepared& result, std::string& error)
{
    stock.clear();
    result = {};
    facade::Preparation output{result, &stock, &error};
    facade::prepare(modules, manifest, deployment, request, output);
    return output.compiled;
}

}

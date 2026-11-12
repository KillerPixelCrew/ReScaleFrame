// SPDX-License-Identifier: GPL-3.0-only
#pragma once
// RSF motion shader variants for the engine's shader load path (create_shader, motion_profile.h).
//
// The engine keeps its stock shader: stock bytecode is created, reflected and its VS input signature extracted
// exactly as before, on the cache-hit and the compile path alike. For an allowlisted root RSF then builds the
// matching motion variant of THAT permutation, before the engine can bind the shader:
//   1. the engine's own compile inputs are read from shader_compile's frame (root source, entry, target, flags and
//      the complete define list; the frame is only trusted when create_shader returns to one of its two calls);
//   2. the root source is identified by SHA-256 in the deployment manifest (never by file name alone);
//   3. the ORIGINAL deployed tree is compiled with those inputs and must reproduce the engine's bytecode byte for
//      byte, which proves root, includes, defines, compiler and flags all match;
//   4. the MOTION tree is compiled with the same inputs;
//   5. the variant's reflection must keep every stock resource and constant-buffer layout and the VS input
//      signature, and may add only the b13 RSF buffer (VS) or SV_Target2 (PS).
// Any failure leaves only the stock shader (counted by reason). Verified variants are cached on disk under a key
// over namespace, root, defines, target, entry, flags and the original bytecode hash; a cache entry is still
// reflection-checked. No shader body is embedded in RSF: both trees are generated locally by
// tools/anomaly_motion_deploy.py from the user's own installation and read from the deployment directory.
#include <array>
#include <cstddef>
#include <cstdint>
#include <filesystem>
#include <functional>
#include <map>
#include <optional>
#include <span>
#include <string>
#include <string_view>
#include <vector>

namespace rsf::anomaly::motion {
enum class Family : std::uint8_t { None, CoreStatic, CoreModel, CorePixel, FoliageTree, FoliageDetail, Count };
const char* family_name(Family) noexcept;
bool family_is_vertex(Family) noexcept;

struct ManifestEntry {
    char stage{};            // 'v' or 'p'
    Family family{};
    std::string name;        // root file name, relative to r3
    std::string original_sha256, motion_sha256;
};
// Line format (tools/anomaly_motion_deploy.py):
//   rsf-anomaly-motion-manifest 1
//   namespace <64 hex>
//   root <vs|ps> <family> <name> <original sha256> <motion sha256>
struct Manifest {
    std::string name_space;
    std::map<std::string, ManifestEntry> by_original;   // keyed by the original root source SHA-256
    static std::optional<Manifest> parse(std::string_view text, std::string& error);
};

std::string sha256_hex(std::span<const std::byte>);

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
// Guarded engine-memory reader (returns false on a fault).
using FrameReader = std::function<bool(const void* address, void* out, std::size_t size)>;
enum class FrameStatus : std::uint8_t { Ok, UnknownCaller, Unreadable, BadSource, BadDefines };
// `return_address` is create_shader's return address, `caller_rsp` the caller's rsp at the call. `module` is the
// executable base. Only the two verified call sites in motion_profile.h are accepted.
FrameStatus read_request(const std::byte* module, const void* return_address, const std::byte* caller_rsp,
    const FrameReader&, ShaderRequest&);

// One compile through the engine's d3dcompiler with an include handler rooted at `tree`/r3.
class Compiler {
public:
    virtual ~Compiler() = default;
    virtual bool compile(const std::filesystem::path& tree, std::span<const std::byte> root, const ShaderRequest&,
        std::vector<std::byte>& bytecode, std::string& error) = 0;
};

// Reflection facts of one shader, enough to check compatibility and stage linkage.
struct Parameter {
    std::string name;
    std::uint32_t index{}, reg{}, system_value{}, component{}, mask{};
    bool operator==(const Parameter&) const = default;
};
struct Variable {
    std::string name;
    std::uint32_t offset{}, size{};
    bool operator==(const Variable&) const = default;
};
struct Buffer {
    std::string name;
    std::uint32_t size{};
    std::vector<Variable> variables;
    bool operator==(const Buffer&) const = default;
};
struct Binding {
    std::string name;
    std::uint32_t type{}, point{}, count{}, return_type{}, dimension{};
    bool operator==(const Binding&) const = default;
};
struct Reflection {
    std::uint32_t version{};
    std::vector<Parameter> inputs, outputs;
    std::vector<Binding> bindings;
    std::vector<Buffer> buffers;
};
class Reflector {
public:
    virtual ~Reflector() = default;
    virtual bool reflect(std::span<const std::byte> bytecode, Reflection&) = 0;
};
// D3D_SIT_CBUFFER = 0.
enum class Compatibility : std::uint8_t { Ok, Version, MissingBinding, ExtraBinding, Buffer, Inputs, Outputs };
const char* compatibility_name(Compatibility) noexcept;
Compatibility compatible(char stage, const Reflection& stock, const Reflection& variant) noexcept;
// Linkage of a variant VS with a variant PS: every PS input must be a VS output with the same name, index and
// register and a covering mask.
bool linkable(const Reflection& vertex, const Reflection& pixel) noexcept;

enum class Outcome : std::uint8_t {
    NotAllowlisted, Replaced, ReplacedFromCache, UnknownCaller, Unreadable, OriginalCompileFailed, OriginalMismatch,
    MotionCompileFailed, ReflectionMismatch, CreateFailed, Count
};
const char* outcome_name(Outcome) noexcept;
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

class ShaderLoader {
public:
    // `deployment` holds manifest.txt, original/r3 and motion/r3. `cache` may be empty (no disk cache).
    ShaderLoader(Manifest, std::filesystem::path deployment, std::filesystem::path cache, Compiler&, Reflector&);
    const Manifest& manifest() const noexcept { return manifest_; }
    // True when the root source is in the manifest; cheap, so the detour can skip everything else.
    const ManifestEntry* lookup(std::span<const std::byte> source) const;
    Prepared prepare(const ShaderRequest&);
    std::string cache_key(const ShaderRequest&, const ManifestEntry&) const;
private:
    Manifest manifest_;
    std::filesystem::path deployment_, cache_;
    Compiler& compiler_;
    Reflector& reflector_;
};
int skin_mode(const std::vector<Define>&) noexcept;
}

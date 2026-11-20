// SPDX-License-Identifier: MIT
#include "runtime_shaders.h"
#include "shader_loader_platform.h"
#include <d3dcompiler.h>
#include <cstring>
#include <fstream>
#include <memory>

namespace rsf::anomaly::motion {
namespace {
// Match the Runtime v1 statuses interpreted by Rust.
static_assert(RSF_SHADER_PATCH_VERSION == 1 && RSF_PATCH_OK == 0 && RSF_PATCH_ORIGINAL == -7 && RSF_PATCH_REFLECTION == -8);

bool read(void* out, const void* address, std::size_t size) noexcept
{
    __try { std::memcpy(out, address, size); return true; }
    __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
}
HRESULT include_open(void* table, const char* name, const void** data, UINT* size) noexcept
{
    __try {
        auto* object = reinterpret_cast<ID3DInclude*>(&table);
        return object->Open(D3D_INCLUDE_LOCAL, name, nullptr, data, size);
    } __except (EXCEPTION_EXECUTE_HANDLER) { return E_FAIL; }
}
void include_close(void* table, const void* data) noexcept
{
    __try { reinterpret_cast<ID3DInclude*>(&table)->Close(data); }
    __except (EXCEPTION_EXECUTE_HANDLER) {}
}
}

RuntimeShaders::~RuntimeShaders()
{
    if (session_) api_.close(session_);
}
bool RuntimeShaders::open(const std::filesystem::path& pack, const std::filesystem::path& cache,
    std::byte* image, const MotionProfile& profile, std::string& error)
{
    if (session_) return false;
    // The guarded D3DCompile setup loads the stateless includer's vtable with lea rax,[rip+disp32].
    const auto* lea = image + profile.d3dcompile_setup_rva + 6;
    unsigned char opcode[3]{};
    int32_t displacement{};
    if (!read(opcode, lea, sizeof(opcode)) || std::memcmp(opcode, "\x48\x8d\x05", sizeof(opcode)) != 0 ||
        !read(&displacement, lea + 3, sizeof(displacement))) {
        error = "include table instruction";
        return false;
    }
    void* table = const_cast<std::byte*>(lea + 7 + displacement);
    void* functions[2]{};
    unsigned char open_bytes[9]{}, close_bytes[8]{};
    if (!read(functions, table, sizeof(functions)) || !read(open_bytes, functions[0], sizeof(open_bytes)) ||
        !read(close_bytes, functions[1], sizeof(close_bytes)) ||
        std::memcmp(open_bytes, "\x40\x53\x48\x81\xec\x40\x02\x00\x00", sizeof(open_bytes)) ||
        std::memcmp(close_bytes, "\x48\x83\xec\x28\x48\x85\xd2\x74", sizeof(close_bytes))) {
        error = "engine includer entries";
        return false;
    }
    const auto runtime = GetModuleHandleW(L"ReScaleFrame.Runtime.dll");
    const auto query = runtime
        ? reinterpret_cast<rsf_get_shader_patch_api_fn>(GetProcAddress(runtime, "rsf_get_shader_patch_api")) : nullptr;
    api_.struct_size = sizeof(api_);
    api_.abi_version = RSF_SHADER_PATCH_VERSION;
    if (!query || query(&api_) != RSF_PATCH_OK) {
        error = "Runtime shader service unavailable";
        return false;
    }
    std::ifstream file(pack, std::ios::binary);
    const std::string data((std::istreambuf_iterator<char>(file)), {});
    const auto compiler = GetModuleHandleW(engine_compiler_module);
    const auto fn = compiler ? GetProcAddress(compiler, "D3DCompile") : nullptr;
    rsf_patch_session* session{};
    const auto status = api_.open({data.data(), data.size()}, cache.c_str(), reinterpret_cast<void*>(fn), &session);
    if (status != RSF_PATCH_OK) {
        error = "Runtime pack refused: " + std::to_string(status);
        return false;
    }
    std::unique_ptr<rsf_patch_session, decltype(api_.close)> pending(session, api_.close);
    struct RootSource { const rsf_shader_patch_api& api; const rsf_patch_session* session; } source{api_, session};
    const auto root = [](const void* user, uint32_t index, rsf_anomaly_shader_root* out) noexcept {
        const auto& reader = *static_cast<const RootSource*>(user);
        rsf_patch_root item{sizeof(item), RSF_SHADER_PATCH_VERSION};
        if (reader.api.root(reader.session, index, &item) != RSF_PATCH_OK) return false;
        const auto bytes = [](const char* value) -> rsf_anomaly_shader_bytes {
            return {reinterpret_cast<const uint8_t*>(value), value ? std::strlen(value) : 0};
        };
        *out = {bytes(item.name), bytes(item.source_sha256), bytes(item.patched_sha256), bytes(item.tag), item.stage};
        return true;
    };
    const auto* pack_hash = api_.hash(session);
    if (!admit_runtime_manifest(manifest, pack_hash ? pack_hash : "", api_.root_count(session), &source, root, error)) return false;
    include_table_ = table;
    session_ = pending.release();
    return true;
}
int32_t RuntimeShaders::resolve(void* user, const char* name, rsf_patch_bytes* out) noexcept try
{
    auto& self = *static_cast<RuntimeShaders*>(user);
    const void* data{};
    UINT size{};
    std::string engine_name(name);
    for (auto& c : engine_name) if (c == '/') c = '\\';
    if (FAILED(include_open(self.include_table_, engine_name.c_str(), &data, &size)) || !data) return RSF_PATCH_SOURCE;
    try { self.source_.resize(size); }
    catch (...) { include_close(self.include_table_, data); return RSF_PATCH_SOURCE; }
    const bool ok = read(self.source_.data(), data, size);
    include_close(self.include_table_, data);
    if (!ok) return RSF_PATCH_SOURCE;
    *out = {self.source_.data(), self.source_.size()};
    return RSF_PATCH_OK;
} catch (...) { return RSF_PATCH_SOURCE; }

Prepared RuntimeShaders::prepare(const ShaderRequest& r, const ManifestEntry& entry, D3DShaderApi& reflector)
{
    std::vector<rsf_patch_define> defines;
    defines.reserve(r.defines.size());
    for (const auto& d : r.defines) defines.push_back({d.name.c_str(), d.value.c_str()});
    const rsf_patch_request request{sizeof(request), RSF_SHADER_PATCH_VERSION, {r.source.data(), r.source.size()},
        {r.bytecode.data(), r.bytecode.size()}, r.entry.c_str(), r.target.c_str(), r.flags, defines.data(), defines.size(), resolve, this};
    rsf_patch_output output{};
    output.struct_size = sizeof(output);
    output.abi_version = RSF_SHADER_PATCH_VERSION;
    const auto status = api_.prepare(session_, &request, &output);
    return finish_runtime_shader(r, entry, reflector, status, output.cache_hit != 0,
        {static_cast<const std::byte*>(output.bytecode.data), output.bytecode.size}, output.reason);
}
}

// SPDX-License-Identifier: MIT
#include "runtime_shaders.h"
#include "shader_loader_platform.h"
#include <d3dcompiler.h>
#include <cstring>

namespace rsf::anomaly::motion {
extern "C" {
RuntimePack* rsf_anomaly_runtime_pack_open(const uint16_t*, const uint16_t*, void*, char*, size_t) noexcept;
void rsf_anomaly_runtime_pack_close(RuntimePack*) noexcept;
const rsf_anomaly_shader_admitted* rsf_anomaly_runtime_pack_manifest(const RuntimePack*) noexcept;
const rsf_shader_patch_api* rsf_anomaly_runtime_pack_api(const RuntimePack*) noexcept;
rsf_patch_session* rsf_anomaly_runtime_pack_session(const RuntimePack*) noexcept;
}
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
    rsf_anomaly_runtime_pack_close(pack_);
}
const rsf_anomaly_shader_admitted* RuntimeShaders::manifest() const noexcept {
    return rsf_anomaly_runtime_pack_manifest(pack_);
}
const rsf_shader_patch_api& RuntimeShaders::api() const noexcept {
    return *rsf_anomaly_runtime_pack_api(pack_);
}

bool RuntimeShaders::open(const std::filesystem::path& pack, const std::filesystem::path& cache,
    std::byte* image, const MotionProfile& profile, std::string& error)
{
    if (pack_) return false;
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
    const auto compiler = GetModuleHandleW(engine_compiler_module);
    const auto fn = compiler ? GetProcAddress(compiler, "D3DCompile") : nullptr;
    char message[512]{};
    pack_ = rsf_anomaly_runtime_pack_open(reinterpret_cast<const uint16_t*>(pack.c_str()),
        reinterpret_cast<const uint16_t*>(cache.c_str()), reinterpret_cast<void*>(fn), message, sizeof(message));
    if (!pack_) {
        error = message;
        return false;
    }
    include_table_ = table;
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
    const auto status = api().prepare(rsf_anomaly_runtime_pack_session(pack_), &request, &output);
    return finish_runtime_shader(r, entry, reflector, status, output.cache_hit != 0,
        {static_cast<const std::byte*>(output.bytecode.data), output.bytecode.size}, output.reason);
}
}

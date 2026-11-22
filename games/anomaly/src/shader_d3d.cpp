// SPDX-License-Identifier: MIT
#include "shader_d3d.h"
#include <new>
#include <utility>

namespace rsf::anomaly::motion {
using OpenFile = HRESULT (*)(const void* tree, const char* prefix, const char* name, HANDLE*);
extern "C" {
ShaderD3DModules* rsf_anomaly_shader_d3d_load(const uint16_t*, FARPROC*, FARPROC*) noexcept;
void rsf_anomaly_shader_d3d_unload(ShaderD3DModules*) noexcept;
bool rsf_anomaly_shader_reflect(FARPROC, rsf_anomaly_shader_bytes, void*,
    bool (*)(void*, const rsf_anomaly_shader_reflection*)) noexcept;
bool rsf_anomaly_shader_compile_tree(FARPROC, const void*, OpenFile, const rsf_anomaly_shader_request*,
    void*, bool (*)(void*, bool, rsf_anomaly_shader_bytes)) noexcept;
}

namespace {
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

std::string name(rsf_anomaly_shader_bytes value)
{
    return value.len ? std::string(reinterpret_cast<const char*>(value.data), value.len) : std::string{};
}

bool copy_reflection(void* user, const rsf_anomaly_shader_reflection* view) noexcept try
{
    Reflection result;
    result.version = view->version;
    const auto signatures = [](const auto& rows, auto& out) {
        out.reserve(rows.len);
        for (size_t i = 0; i < rows.len; ++i) {
            const auto& p = rows.data[i];
            out.push_back({name(p.name), p.index, p.reg, p.system_value, p.component, p.mask});
        }
    };
    signatures(view->inputs, result.inputs);
    signatures(view->outputs, result.outputs);
    signatures(view->patch, result.patch);
    result.bindings.reserve(view->bindings.len);
    for (size_t i = 0; i < view->bindings.len; ++i) {
        const auto& b = view->bindings.data[i];
        result.bindings.push_back({name(b.name), b.type, b.point, b.count, b.return_type, b.dimension});
    }
    result.buffers.reserve(view->buffers.len);
    for (size_t i = 0; i < view->buffers.len; ++i) {
        const auto& b = view->buffers.data[i];
        Buffer buffer{name(b.name), b.size, {}};
        buffer.variables.reserve(b.variables.len);
        for (size_t j = 0; j < b.variables.len; ++j) {
            const auto& v = b.variables.data[j];
            buffer.variables.push_back({name(v.name), v.offset, v.size});
        }
        result.buffers.push_back(std::move(buffer));
    }
    *static_cast<Reflection*>(user) = std::move(result);
    return true;
} catch (...) { return false; }
}

D3DShaderApi::D3DShaderApi(const wchar_t* name) noexcept
{
    static_assert(sizeof(wchar_t) == sizeof(uint16_t));
    modules_ = rsf_anomaly_shader_d3d_load(reinterpret_cast<const uint16_t*>(name), &compile_, &reflect_);
}

D3DShaderApi::~D3DShaderApi()
{
    rsf_anomaly_shader_d3d_unload(modules_);
}

bool D3DShaderApi::compile(const std::filesystem::path& tree, std::span<const std::byte> root, const ShaderRequest& request,
    std::vector<std::byte>& bytecode, std::string& error)
{
    bytecode.clear();
    const auto text = [](std::string_view value) -> rsf_anomaly_shader_bytes {
        return {reinterpret_cast<const uint8_t*>(value.data()), value.size()};
    };
    std::vector<rsf_anomaly_shader_define> defines;
    defines.reserve(request.defines.size());
    for (const auto& d : request.defines) defines.push_back({text(d.name), text(d.value)});
    const rsf_anomaly_shader_request input{text(request.target), text(request.entry), request.flags,
        {defines.data(), defines.size()}, {reinterpret_cast<const uint8_t*>(root.data()), root.size()}, {}};
    struct Output { std::vector<std::byte>& code; std::string& error; } output{bytecode, error};
    return rsf_anomaly_shader_compile_tree(compile_, &tree, open_file, &input, &output,
        [](void* user, bool ok, rsf_anomaly_shader_bytes value) noexcept {
            try {
                auto& out = *static_cast<Output*>(user);
                if (!ok) {
                    out.error = name(value);
                    return false;
                }
                if (value.len) {
                    const auto* start = reinterpret_cast<const std::byte*>(value.data);
                    out.code.assign(start, start + value.len);
                }
                return true;
            } catch (...) { return false; }
        });
}

bool D3DShaderApi::reflect(std::span<const std::byte> bytecode, Reflection& out)
{
    out = {};
    return rsf_anomaly_shader_reflect(reflect_, {reinterpret_cast<const uint8_t*>(bytecode.data()), bytecode.size()},
        &out, copy_reflection);
}
}

// First-party D3D11 fixtures. Executes local FXC-compiled game shaders, not a game.
#include <d3d11.h>
#include <d3dcompiler.h>
#include <d3d11sdklayers.h>
#include <wrl/client.h>
#include <array>
#include <bit>
#include <cmath>
#include <cstdint>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <sstream>
#include <stdexcept>
#include <string>
#include <vector>
using Microsoft::WRL::ComPtr;
using V4 = std::array<float, 4>;
using Matrix = std::array<V4, 4>;
static ComPtr<ID3D11Device> device;
static ComPtr<ID3D11DeviceContext> context;
static unsigned vs_checks = 0, ps_checks = 0, invalid_checks = 0, zero_motion_checks = 0;

static void require(bool ok, const std::string& message) {
    if (!ok) throw std::runtime_error(message);
}
static void hr(HRESULT value, const char* message) {
    if (FAILED(value)) throw std::runtime_error(std::string(message) + " HRESULT=" + std::to_string(value));
}
static void gpu_errors() {
    ComPtr<ID3D11InfoQueue> queue;
    if (FAILED(device.As(&queue))) return;
    const UINT64 count = queue->GetNumStoredMessagesAllowedByRetrievalFilter();
    for (UINT64 i = 0; i < count; ++i) {
        SIZE_T size = 0; queue->GetMessage(i, nullptr, &size);
        std::vector<char> storage(size);
        auto message = reinterpret_cast<D3D11_MESSAGE*>(storage.data());
        hr(queue->GetMessage(i, message, &size), "info queue");
        if (message->Severity <= D3D11_MESSAGE_SEVERITY_ERROR) throw std::runtime_error(message->pDescription);
    }
    queue->ClearStoredMessages();
}
static std::vector<char> read(const std::filesystem::path& path) {
    std::ifstream input(path, std::ios::binary);
    require(input.good(), "cannot open " + path.string());
    return {std::istreambuf_iterator<char>(input), std::istreambuf_iterator<char>()};
}
static ComPtr<ID3D11ShaderReflection> reflect(const std::vector<char>& bytes) {
    ComPtr<ID3D11ShaderReflection> result;
    hr(D3DReflect(bytes.data(), bytes.size(), IID_PPV_ARGS(&result)), "D3DReflect");
    return result;
}
static ComPtr<ID3DBlob> compile(const std::string& source, const char* target) {
    ComPtr<ID3DBlob> code, error;
    const HRESULT result = D3DCompile(source.data(), source.size(), "first-party-fixture", nullptr, nullptr,
        "main", target, D3DCOMPILE_ENABLE_STRICTNESS, 0, &code, &error);
    if (FAILED(result) && error) throw std::runtime_error(static_cast<const char*>(error->GetBufferPointer()));
    hr(result, "fixture compile");
    return code;
}
static Matrix matrix(bool previous, bool hud = false) {
    if (previous) return {{{0.8f, 0.07f, 0, hud ? -0.4f : 0.12f},
                          {0.03f, 1.1f, 0.04f, -0.08f},
                          {0, 0.05f, 0.9f, 0.02f},
                          {0.04f, 0.02f, 0.01f, hud ? 0.75f : 1.15f}}};
    return {{{1.2f, 0.02f, 0.03f, 0.1f}, {0.01f, 0.9f, 0.02f, -0.05f},
             {0.02f, 0.03f, 1.1f, 0.04f}, {0.1f, 0.03f, 0.02f, 1.1f}}};
}
static V4 mul(const Matrix& m, const V4& v) {
    V4 result{};
    for (unsigned row = 0; row < 4; ++row)
        for (unsigned col = 0; col < 4; ++col) result[row] += m[row][col] * v[col];
    return result;
}
static std::array<V4, 234> palette(bool previous) {
    std::array<V4, 234> rows{};
    for (unsigned bone = 0; bone < 78; ++bone) {
        const float n = static_cast<float>(bone);
        const float offset = previous ? 0.13f : 0.02f;
        rows[3 * bone] = {0.9f + 0.01f * n, 0.02f, 0.01f, offset + 0.01f * n};
        rows[3 * bone + 1] = {0.01f, 1.05f + 0.01f * n, 0.03f, -offset - 0.02f * n};
        rows[3 * bone + 2] = {0.03f, 0.01f, 0.95f, offset + 0.03f * n};
    }
    return rows;
}
static V4 deformed(const std::string& mode, bool previous) {
    V4 point{0.1f, -0.2f, 0.3f, 0.7f};
    if (mode == "STATIC" || mode == "NONE") return point;
    point[3] = 1;
    if (mode == "0") return point;
    std::array<unsigned, 4> ids{};
    V4 weights{};
    if (mode == "1") { ids = {3, 0, 0, 0}; weights = {1, 0, 0, 0}; }
    if (mode == "2") { ids = {3, 6, 0, 0}; weights = {0.8f, 0.2f, 0, 0}; }
    if (mode == "3") { ids = {3, 6, 9, 0}; weights = {0.2f, 0.3f, 0.5f, 0}; }
    // Packed D3DCOLOR index xyz swizzle: raw {12,9,6,3} -> {6,9,12,3}.
    if (mode == "4") { ids = {6, 9, 12, 3}; weights = {0.2f, 0.3f, 0.1f, 0.4f}; }
    auto bones = palette(previous);
    V4 result{0, 0, 0, 1};
    for (unsigned row = 0; row < 3; ++row)
        for (unsigned i = 0; i < 4; ++i)
            for (unsigned col = 0; col < 4; ++col)
                result[row] += bones[ids[i] + row][col] * weights[i] * point[col];
    return result;
}
static void close(float actual, float expected, const std::string& name, float tolerance = 0.00002f) {
    require(std::isfinite(actual) && std::isfinite(expected) && std::abs(actual - expected) <= tolerance,
            name + ": actual=" + std::to_string(actual) + " expected=" + std::to_string(expected));
}
static ComPtr<ID3D11Buffer> buffer(const void* data, UINT size, UINT bind, UINT usage = D3D11_USAGE_DEFAULT,
                                  UINT cpu = 0) {
    D3D11_BUFFER_DESC description{};
    description.ByteWidth = size; description.BindFlags = bind;
    description.Usage = static_cast<D3D11_USAGE>(usage); description.CPUAccessFlags = cpu;
    D3D11_SUBRESOURCE_DATA init{}; init.pSysMem = data;
    ComPtr<ID3D11Buffer> result;
    hr(device->CreateBuffer(&description, data ? &init : nullptr, &result), "CreateBuffer");
    return result;
}
static void constants(ID3D11ShaderReflection* reflection, bool vertex, bool previous, bool hud,
                      V4 history = {1, 1, 1, 1}, bool reject_alpha = false) {
    D3D11_SHADER_DESC shader{}; hr(reflection->GetDesc(&shader), "reflection desc");
    for (UINT i = 0; i < shader.BoundResources; ++i) {
        D3D11_SHADER_INPUT_BIND_DESC binding{}; hr(reflection->GetResourceBindingDesc(i, &binding), "binding desc");
        if (binding.Type != D3D_SIT_CBUFFER) continue;
        auto cb = reflection->GetConstantBufferByName(binding.Name);
        D3D11_SHADER_BUFFER_DESC desc{}; hr(cb->GetDesc(&desc), "cb desc");
        std::vector<float> values((desc.Size + 15) / 16 * 4, 0.1f);
        for (UINT j = 0; j < desc.Variables; ++j) {
            auto variable = cb->GetVariableByIndex(j);
            D3D11_SHADER_VARIABLE_DESC vd{}; hr(variable->GetDesc(&vd), "variable desc");
            const std::string name = vd.Name;
            float* target = values.data() + vd.StartOffset / 4;
            D3D11_SHADER_TYPE_DESC type{}; hr(variable->GetType()->GetDesc(&type), "type desc");
            if (name == "rsf_previous_wvp") {
                auto m = matrix(true, hud); std::memcpy(target, &m, sizeof(m));
            } else if (name == "rsf_previous_bones" || name == "sbones_array") {
                auto bones = palette(name == "rsf_previous_bones"); std::memcpy(target, &bones, sizeof(bones));
            } else if (name == "rsf_history") std::memcpy(target, &history, sizeof(history));
            else if (name == "rsf_viewport") { const V4 v{0, 0, 1.0f / 16, 1.0f / 16}; std::memcpy(target, &v, 16); }
            else if (name == "rsf_jitter") { const V4 v{0.01f, -0.02f, -0.03f, 0.04f}; std::memcpy(target, &v, 16); }
            else if (name == "rsf_pixel_control") target[0] = 1;
            else if (name == "alpha_ref" || name == "def_aref") target[0] = reject_alpha ? 1.1f : 0.5f;
            else if (type.Class == D3D_SVC_MATRIX_COLUMNS || type.Class == D3D_SVC_MATRIX_ROWS) {
                Matrix m{}; for (unsigned k = 0; k < 4; ++k) m[k][k] = 1;
                if (name == "m_WVP") m = matrix(false);
                if (type.Class == D3D_SVC_MATRIX_COLUMNS) {
                    for (UINT col = 0; col < type.Columns; ++col)
                        for (UINT row = 0; row < type.Rows; ++row) target[col * 4 + row] = m[row][col];
                } else {
                    for (UINT row = 0; row < type.Rows; ++row)
                        for (UINT col = 0; col < type.Columns; ++col) target[row * 4 + col] = m[row][col];
                }
            }
        }
        require((binding.BindPoint == 13) == (std::string(binding.Name).starts_with("RSFCore")), "b13 collision");
        auto cb_buffer = buffer(values.data(), static_cast<UINT>(values.size() * 4), D3D11_BIND_CONSTANT_BUFFER);
        auto pointer = cb_buffer.Get();
        if (vertex) context->VSSetConstantBuffers(binding.BindPoint, 1, &pointer);
        else context->PSSetConstantBuffers(binding.BindPoint, 1, &pointer);
    }
    (void)previous;
}
static std::vector<D3D11_SIGNATURE_PARAMETER_DESC> outputs(ID3D11ShaderReflection* reflection) {
    D3D11_SHADER_DESC desc{}; hr(reflection->GetDesc(&desc), "shader desc");
    std::vector<D3D11_SIGNATURE_PARAMETER_DESC> result(desc.OutputParameters);
    for (UINT i = 0; i < desc.OutputParameters; ++i) hr(reflection->GetOutputParameterDesc(i, &result[i]), "output desc");
    return result;
}
static std::vector<float> run_vs(const std::vector<char>& bytes,
        const std::vector<D3D11_SIGNATURE_PARAMETER_DESC>& original, const std::string& mode,
        bool augmented, bool hud = false, V4 history = {1, 1, 1, 1}) {
    context->ClearState();
    auto reflection = reflect(bytes);
    D3D11_SHADER_DESC desc{}; hr(reflection->GetDesc(&desc), "VS desc");
    std::vector<D3D11_INPUT_ELEMENT_DESC> layout;
    std::vector<V4> vertices(desc.InputParameters, V4{0.4f, 0.5f, 0.6f, 0.2f});
    std::vector<std::string> names; names.reserve(desc.InputParameters);
    const DXGI_FORMAT formats[3][4] = {
        {DXGI_FORMAT_R32_UINT, DXGI_FORMAT_R32G32_UINT, DXGI_FORMAT_R32G32B32_UINT, DXGI_FORMAT_R32G32B32A32_UINT},
        {DXGI_FORMAT_R32_SINT, DXGI_FORMAT_R32G32_SINT, DXGI_FORMAT_R32G32B32_SINT, DXGI_FORMAT_R32G32B32A32_SINT},
        {DXGI_FORMAT_R32_FLOAT, DXGI_FORMAT_R32G32_FLOAT, DXGI_FORMAT_R32G32B32_FLOAT, DXGI_FORMAT_R32G32B32A32_FLOAT}};
    for (UINT i = 0; i < desc.InputParameters; ++i) {
        D3D11_SIGNATURE_PARAMETER_DESC in{}; hr(reflection->GetInputParameterDesc(i, &in), "input desc");
        names.emplace_back(in.SemanticName);
        const UINT count = std::popcount(static_cast<unsigned>(in.Mask));
        const UINT category = in.ComponentType == D3D_REGISTER_COMPONENT_UINT32 ? 0u :
                              in.ComponentType == D3D_REGISTER_COMPONENT_SINT32 ? 1u : 2u;
        layout.push_back({names.back().c_str(), in.SemanticIndex, formats[category][count - 1], 0,
                          i * 16, D3D11_INPUT_PER_VERTEX_DATA, 0});
        auto& v = vertices[i];
        if (names.back() == "POSITION") v = {0.1f, -0.2f, 0.3f, 0.7f};
        if (names.back() == "NORMAL") v[3] = mode == "1" ? 3.0f / 255 : 0.2f;
        if (names.back() == "TANGENT") v[3] = 0.3f;
        if (names.back() == "BINORMAL") v[3] = mode == "3" ? 9.0f / 255 : 0.1f;
        if (names.back() == "TEXCOORD") {
            v = in.SemanticIndex == 1 ? V4{12.0f / 255, 9.0f / 255, 6.0f / 255, 3.0f / 255} : V4{0.1f, 0.2f, 3, 6};
            if (category != 2) { const std::array<int, 4> integer{32, 64, 3, 6}; std::memcpy(&v, &integer, 16); }
        }
    }
    ComPtr<ID3D11InputLayout> il;
    hr(device->CreateInputLayout(layout.data(), static_cast<UINT>(layout.size()), bytes.data(), bytes.size(), &il), "input layout");
    auto vb = buffer(vertices.data(), static_cast<UINT>(vertices.size() * 16), D3D11_BIND_VERTEX_BUFFER);
    UINT stride = static_cast<UINT>(vertices.size() * 16), offset = 0;
    auto vertex_buffer = vb.Get(); context->IASetVertexBuffers(0, 1, &vertex_buffer, &stride, &offset);
    context->IASetInputLayout(il.Get()); context->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_POINTLIST);
    ComPtr<ID3D11VertexShader> vs; hr(device->CreateVertexShader(bytes.data(), bytes.size(), nullptr, &vs), "create real VS");
    context->VSSetShader(vs.Get(), nullptr, 0); constants(reflection.Get(), true, augmented, hud, history);
    std::vector<D3D11_SO_DECLARATION_ENTRY> so;
    UINT floats = 0;
    for (const auto& out : original) {
        const BYTE count = static_cast<BYTE>(std::popcount(static_cast<unsigned>(out.Mask)));
        so.push_back({0, out.SemanticName, out.SemanticIndex, 0, count, 0}); floats += count;
    }
    if (augmented) { so.push_back({0, "TEXCOORD", 12, 0, 4, 0}); so.push_back({0, "TEXCOORD", 13, 0, 1, 0}); floats += 5; }
    const UINT so_stride = floats * 4;
    ComPtr<ID3D11GeometryShader> gs;
    // D3D11 supports stream output directly from vertex bytecode.
    hr(device->CreateGeometryShaderWithStreamOutput(bytes.data(), bytes.size(), so.data(),
        static_cast<UINT>(so.size()), &so_stride, 1, D3D11_SO_NO_RASTERIZED_STREAM, nullptr, &gs), "VS stream output");
    context->GSSetShader(gs.Get(), nullptr, 0);
    auto capture = buffer(nullptr, so_stride, D3D11_BIND_STREAM_OUTPUT);
    auto target = capture.Get(); context->SOSetTargets(1, &target, &offset);
    context->Draw(1, 0); context->SOSetTargets(0, nullptr, nullptr);
    auto staging = buffer(nullptr, so_stride, 0, D3D11_USAGE_STAGING, D3D11_CPU_ACCESS_READ);
    context->CopyResource(staging.Get(), capture.Get());
    D3D11_MAPPED_SUBRESOURCE mapped{}; hr(context->Map(staging.Get(), 0, D3D11_MAP_READ, 0, &mapped), "map VS output");
    std::vector<float> result(floats); std::memcpy(result.data(), mapped.pData, so_stride); context->Unmap(staging.Get(), 0);
    return result;
}
static void test_vs(const std::vector<char>& old_code, const std::vector<char>& new_code, const std::string& mode) {
    auto reflection = reflect(old_code); auto out = outputs(reflection.Get());
    auto before = run_vs(old_code, out, mode, false);
    for (const bool hud : {false, true}) {
        auto after = run_vs(new_code, out, mode, true, hud);
        require(after.size() == before.size() + 5, "VS stream size");
        for (size_t i = 0; i < before.size(); ++i) close(after[i], before[i], "native output unchanged");
        auto current = mul(matrix(false), deformed(mode, false));
        auto previous = mul(matrix(true, hud), deformed(mode, true));
        size_t index = 0;
        for (const auto& semantic : out) {
            if (semantic.SystemValueType == D3D_NAME_POSITION)
                for (unsigned i = 0; i < 4; ++i) close(after[index + i], current[i], "CPU current clip");
            index += std::popcount(static_cast<unsigned>(semantic.Mask));
        }
        for (unsigned i = 0; i < 4; ++i) close(after[before.size() + i], previous[i], "CPU prior clip");
        close(after.back(), 1, "valid prior"); ++vs_checks;
    }
    V4 flags{0, 1, 1, 1};
    if (mode == "NONE") flags = {1, 1, 0, 1};
    else if (mode != "0" && mode != "STATIC") flags = {1, 0, 1, 1};
    close(run_vs(new_code, out, mode, true, false, flags).back(), 0, "missing prior invalid"); ++invalid_checks;
}

static std::string pixel_fixture_vs(ID3D11ShaderReflection* reflection, bool augmented, float valid, float previous_w, bool stationary) {
    D3D11_SHADER_DESC desc{}; hr(reflection->GetDesc(&desc), "PS desc");
    std::string declaration = "struct O{";
    std::string assignments;
    for (UINT i = 0; i < desc.InputParameters; ++i) {
        D3D11_SIGNATURE_PARAMETER_DESC in{}; hr(reflection->GetInputParameterDesc(i, &in), "PS input");
        if (in.SystemValueType == D3D_NAME_POSITION) continue;
        const std::string field = "f" + std::to_string(i);
        const unsigned count = std::popcount(static_cast<unsigned>(in.Mask));
        const bool validity = std::string(in.SemanticName) == "TEXCOORD" && in.SemanticIndex == 13;
        declaration += std::string(validity ? "nointerpolation " : "") + "float" + std::to_string(count) + " " + field + ":" + in.SemanticName + std::to_string(in.SemanticIndex) + ";";
        std::string value = "float4(0.5,0.5,0.5,0.5)";
        if (std::string(in.SemanticName) == "TEXCOORD") {
            if (in.SemanticIndex == 1) value = "float4(0.1,0.2,2,0.7)";
            if (in.SemanticIndex == 2) value = "float4(0,0,1,0)";
            if (in.SemanticIndex == 3) value = "float4(0,1,0,0)";
            if (in.SemanticIndex == 4) value = "float4(1,0,0,0)";
            if (in.SemanticIndex == 12) {
                const std::string xy = stationary ? "(p.x-0.08)*w,(p.y-0.12)*w" : "(p.x-0.2)*w,(p.y+0.4)*w";
                value = "float4(" + xy + ",0.5*w," + std::to_string(previous_w) + "*w)";
            }
            if (validity) value = "float4(" + std::to_string(valid) + ",0,0,0)";
        }
        assignments += "o." + field + "=(" + value + ")." + std::string("xyzw").substr(0, count) + ";";
    }
    (void)augmented;
    return declaration + "float4 p:SV_Position;};O main(uint id:SV_VertexID){float2 p=float2((id==2)?3:-1,(id==1)?3:-1);float w=1+id;O o;o.p=float4(p*w,0.5*w,w);" + assignments + "return o;}";
}
struct Target {
    ComPtr<ID3D11Texture2D> texture;
    ComPtr<ID3D11RenderTargetView> rtv;
    ComPtr<ID3D11ShaderResourceView> srv;
};
static Target target(DXGI_FORMAT format, UINT samples) {
    D3D11_TEXTURE2D_DESC desc{}; desc.Width = 16; desc.Height = 16; desc.MipLevels = 1; desc.ArraySize = 1;
    desc.Format = format; desc.SampleDesc.Count = samples; desc.BindFlags = D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE;
    Target result; hr(device->CreateTexture2D(&desc, nullptr, &result.texture), "create MRT");
    hr(device->CreateRenderTargetView(result.texture.Get(), nullptr, &result.rtv), "create RTV");
    hr(device->CreateShaderResourceView(result.texture.Get(), nullptr, &result.srv), "create RTV SRV");
    return result;
}
static std::vector<V4> samples_read(Target& input, UINT samples) {
    const UINT count = 256 * samples;
    std::string source = samples == 1 ? "Texture2D<float4> t:register(t0);" :
        "Texture2DMS<float4," + std::to_string(samples) + "> t:register(t0);";
    source += "RWStructuredBuffer<float4> outp:register(u0);[numthreads(16,16,1)]void main(uint3 id:SV_DispatchThreadID){";
    source += "for(uint s=0;s<" + std::to_string(samples) + ";++s)outp[(id.y*16+id.x)*" + std::to_string(samples) + "+s]=";
    source += samples == 1 ? "t.Load(int3(id.xy,0));}" : "t.Load(id.xy,s);}";
    auto bytes = compile(source, "cs_5_0"); ComPtr<ID3D11ComputeShader> shader;
    hr(device->CreateComputeShader(bytes->GetBufferPointer(), bytes->GetBufferSize(), nullptr, &shader), "readback CS");
    D3D11_BUFFER_DESC desc{}; desc.ByteWidth = count * 16; desc.Usage = D3D11_USAGE_DEFAULT;
    desc.BindFlags = D3D11_BIND_UNORDERED_ACCESS; desc.MiscFlags = D3D11_RESOURCE_MISC_BUFFER_STRUCTURED; desc.StructureByteStride = 16;
    ComPtr<ID3D11Buffer> output; hr(device->CreateBuffer(&desc, nullptr, &output), "sample readback buffer");
    ComPtr<ID3D11UnorderedAccessView> uav; hr(device->CreateUnorderedAccessView(output.Get(), nullptr, &uav), "sample UAV");
    auto srv = input.srv.Get(); auto view = uav.Get();
    context->CSSetShader(shader.Get(), nullptr, 0); context->CSSetShaderResources(0, 1, &srv);
    context->CSSetUnorderedAccessViews(0, 1, &view, nullptr); context->Dispatch(1, 1, 1);
    srv = nullptr; view = nullptr; context->CSSetShaderResources(0, 1, &srv); context->CSSetUnorderedAccessViews(0, 1, &view, nullptr);
    auto staging = buffer(nullptr, count * 16, 0, D3D11_USAGE_STAGING, D3D11_CPU_ACCESS_READ);
    context->CopyResource(staging.Get(), output.Get());
    D3D11_MAPPED_SUBRESOURCE mapped{}; hr(context->Map(staging.Get(), 0, D3D11_MAP_READ, 0, &mapped), "map MRT samples");
    std::vector<V4> result(count); std::memcpy(result.data(), mapped.pData, count * 16); context->Unmap(staging.Get(), 0);
    gpu_errors();
    return result;
}
using Pixels = std::array<std::vector<V4>, 3>;
static Pixels run_ps(const std::vector<char>& code, bool augmented, UINT samples,
                     float validity = 1, float previous_w = 1, float alpha = 1, bool stationary = false) {
    context->ClearState();
    auto reflection = reflect(code);
    auto vs_code = compile(pixel_fixture_vs(reflection.Get(), augmented, validity, previous_w, stationary), "vs_5_0");
    ComPtr<ID3D11VertexShader> vs; hr(device->CreateVertexShader(vs_code->GetBufferPointer(), vs_code->GetBufferSize(), nullptr, &vs), "fixture VS");
    ComPtr<ID3D11PixelShader> ps; hr(device->CreatePixelShader(code.data(), code.size(), nullptr, &ps), "real PS");
    context->VSSetShader(vs.Get(), nullptr, 0); context->PSSetShader(ps.Get(), nullptr, 0);
    constants(reflection.Get(), false, false, false);
    D3D11_SHADER_DESC desc{}; hr(reflection->GetDesc(&desc), "PS shader desc");
    std::vector<ComPtr<ID3D11ShaderResourceView>> resources;
    std::vector<ComPtr<ID3D11SamplerState>> samplers;
    for (UINT i = 0; i < desc.BoundResources; ++i) {
        D3D11_SHADER_INPUT_BIND_DESC binding{}; hr(reflection->GetResourceBindingDesc(i, &binding), "PS resource");
        if (binding.Type == D3D_SIT_TEXTURE) {
            require(binding.Dimension == D3D_SRV_DIMENSION_TEXTURE2D, "unexpected material resource dimension");
            D3D11_TEXTURE2D_DESC td{}; td.Width = 2; td.Height = 2; td.MipLevels = 1; td.ArraySize = 1;
            td.Format = DXGI_FORMAT_R32G32B32A32_FLOAT; td.SampleDesc.Count = 1; td.BindFlags = D3D11_BIND_SHADER_RESOURCE;
            std::array<V4, 4> texels{};
            for (auto& texel : texels) texel = {0.5f, 0.4f, 0.6f, std::string(binding.Name) == "s_base" ? alpha : 1.0f};
            D3D11_SUBRESOURCE_DATA init{}; init.pSysMem = texels.data(); init.SysMemPitch = 32;
            ComPtr<ID3D11Texture2D> texture; hr(device->CreateTexture2D(&td, &init, &texture), "fixture texture");
            ComPtr<ID3D11ShaderResourceView> srv; hr(device->CreateShaderResourceView(texture.Get(), nullptr, &srv), "fixture SRV");
            auto pointer = srv.Get(); context->PSSetShaderResources(binding.BindPoint, 1, &pointer); resources.push_back(srv);
        } else if (binding.Type == D3D_SIT_SAMPLER) {
            D3D11_SAMPLER_DESC sd{}; sd.Filter = D3D11_FILTER_MIN_MAG_MIP_LINEAR;
            sd.AddressU = sd.AddressV = sd.AddressW = D3D11_TEXTURE_ADDRESS_CLAMP; sd.MaxLOD = D3D11_FLOAT32_MAX;
            ComPtr<ID3D11SamplerState> sampler; hr(device->CreateSamplerState(&sd, &sampler), "fixture sampler");
            auto pointer = sampler.Get(); context->PSSetSamplers(binding.BindPoint, 1, &pointer); samplers.push_back(sampler);
        }
    }
    std::array<Target, 3> targets{target(DXGI_FORMAT_R32G32B32A32_FLOAT, samples),
        target(DXGI_FORMAT_R32G32B32A32_FLOAT, samples), target(DXGI_FORMAT_R16G16B16A16_FLOAT, samples)};
    const float sentinel[4] = {-999, -999, -999, -999}; const float zero[4]{};
    for (unsigned i = 0; i < 3; ++i) context->ClearRenderTargetView(targets[i].rtv.Get(), i == 2 ? zero : sentinel);
    std::array<ID3D11RenderTargetView*, 3> views{targets[0].rtv.Get(), targets[1].rtv.Get(), targets[2].rtv.Get()};
    context->OMSetRenderTargets(augmented ? 3u : 2u, views.data(), nullptr);
    D3D11_RASTERIZER_DESC rd{}; rd.FillMode = D3D11_FILL_SOLID; rd.CullMode = D3D11_CULL_NONE; rd.DepthClipEnable = true;
    rd.MultisampleEnable = samples > 1;
    ComPtr<ID3D11RasterizerState> rasterizer; hr(device->CreateRasterizerState(&rd, &rasterizer), "rasterizer");
    context->RSSetState(rasterizer.Get()); D3D11_VIEWPORT viewport{0, 0, 16, 16, 0, 1}; context->RSSetViewports(1, &viewport);
    context->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST); context->Draw(3, 0);
    context->OMSetRenderTargets(0, nullptr, nullptr);
    return {samples_read(targets[0], samples), samples_read(targets[1], samples), samples_read(targets[2], samples)};
}
static void test_ps(const std::vector<char>& before, const std::vector<char>& after, UINT samples, unsigned alpha_test) {
    for (const float alpha : (alpha_test ? std::vector<float>{1, 0.45f, 0} : std::vector<float>{1})) {
        auto original = run_ps(before, false, samples, 1, 1, alpha);
        auto augmented = run_ps(after, true, samples, 1, 1, alpha);
        unsigned covered = 0;
        for (size_t i = 0; i < original[0].size(); ++i) {
            for (unsigned mrt = 0; mrt < 2; ++mrt)
                for (unsigned c = 0; c < 4; ++c) {
                    const float a = augmented[mrt][i][c], b = original[mrt][i][c];
                    // Native packed hemi/material deliberately reinterprets bits as float.
                    if (std::bit_cast<uint32_t>(a) != std::bit_cast<uint32_t>(b))
                        close(a, b, "native MRT/coverage unchanged");
                }
            const bool valid = original[0][i][2] != -999;
            if (valid) {
                if (augmented[2][i][2] != 1) {
                    throw std::runtime_error("motion invalid at pixel " + std::to_string(i) + " alpha " + std::to_string(alpha) +
                        " vec=" + std::to_string(augmented[2][i][0]) + "," + std::to_string(augmented[2][i][1]) + "," +
                        std::to_string(augmented[2][i][2]) + "," + std::to_string(augmented[2][i][3]));
                }
                close(augmented[2][i][0], 0.06f, "UV X motion/jitter", 0.0003f);
                close(augmented[2][i][1], 0.26f, "UV Y down motion/jitter", 0.0003f);
                close(augmented[2][i][2], 1, "motion validity"); ++covered;
            } else for (const float c : augmented[2][i]) close(c, 0, "uncovered sample motion");
        }
        if (alpha == 1) require(covered > 0, "fixture produced no covered samples");
        if (alpha_test == 1 && alpha == 0) require(covered == 0, "alpha rejection fixture failed");
        ++ps_checks;
    }
    auto stationary = run_ps(after, true, samples, 1, 1, 1, true);
    unsigned stationary_covered = 0;
    for (size_t i = 0; i < stationary[0].size(); ++i) {
        if (stationary[0][i][2] == -999) continue;
        close(stationary[2][i][0], 0, "stationary jitter cancellation X", 0.0003f);
        close(stationary[2][i][1], 0, "stationary jitter cancellation Y", 0.0003f);
        close(stationary[2][i][2], 1, "zero motion is valid coverage");
        close(stationary[2][i][3], 0, "reserved motion W"); ++stationary_covered;
    }
    require(stationary_covered > 0, "stationary fixture has no coverage"); ++zero_motion_checks;
    for (const auto condition : {V4{0, 1, 0, 0}, V4{1, -1, 0, 0}}) {
        auto invalid = run_ps(after, true, samples, condition[0], condition[1]);
        for (const auto& pixel : invalid[2]) for (const float c : pixel) close(c, 0, "invalid prior coverage");
        ++invalid_checks;
    }
}
int main(int argc, char** argv) {
    try {
        require(argc == 3, "usage: fixture DXBC_DIR CASES_TSV");
        D3D_FEATURE_LEVEL level{}; bool warp = false;
        HRESULT result = D3D11CreateDevice(nullptr, D3D_DRIVER_TYPE_HARDWARE, nullptr, D3D11_CREATE_DEVICE_DEBUG, nullptr, 0,
            D3D11_SDK_VERSION, &device, &level, &context);
        if (FAILED(result)) {
            warp = true; hr(D3D11CreateDevice(nullptr, D3D_DRIVER_TYPE_WARP, nullptr, 0, nullptr, 0,
                D3D11_SDK_VERSION, &device, &level, &context), "D3D11 device");
        }
        require(level >= D3D_FEATURE_LEVEL_11_0, "feature level 11 required");
        std::ifstream cases(argv[2]); require(cases.good(), "case list missing");
        std::string line;
        while (std::getline(cases, line)) {
            std::istringstream stream(line); unsigned index; std::string stage, mode; UINT samples = 1; unsigned alpha = 0;
            stream >> index >> stage >> mode >> samples >> alpha;
            const auto root = std::filesystem::path(argv[1]);
            auto before = read(root / (std::to_string(index) + "-old.dxbc"));
            auto after = read(root / (std::to_string(index) + "-new.dxbc"));
            try {
                if (stage == "vs") test_vs(before, after, mode);
                else test_ps(before, after, samples, alpha);
            } catch (const std::exception& error) { throw std::runtime_error("case " + std::to_string(index) + " " + error.what()); }
        }
        std::cout << "{\"device\":\"" << (warp ? "WARP" : "hardware") << "\",\"vs_checks\":" << vs_checks
                  << ",\"ps_checks\":" << ps_checks << ",\"invalid_checks\":" << invalid_checks
                  << ",\"zero_motion_checks\":" << zero_motion_checks << "}\n";
        return 0;
    } catch (const std::exception& error) { std::cerr << error.what() << '\n'; return 1; }
}

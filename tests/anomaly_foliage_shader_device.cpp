// First-party D3D11 fixture. Executes FXC-built local assets, not helper math.
#include <windows.h>
#include <d3d11.h>
#include <d3dcompiler.h>
#include <d3d11shader.h>
#include <dxgi.h>
#include <array>
#include <cmath>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <map>
#include <stdexcept>
#include <string>
#include <utility>
#include <vector>

namespace {
template<class T> struct Com {
    T* p = nullptr;
    Com() = default;
    Com(const Com&) = delete;
    Com& operator=(const Com&) = delete;
    Com(Com&& other) noexcept : p(std::exchange(other.p, nullptr)) {}
    ~Com() { if (p) p->Release(); }
    T** put() { if (p) p->Release(); p = nullptr; return &p; }
    T* operator->() const { return p; }
};
void check(HRESULT result, const char* operation) {
    if (FAILED(result)) throw std::runtime_error(std::string(operation) + " HRESULT=" + std::to_string(result));
}
void require(bool value, const std::string& message) {
    if (!value) throw std::runtime_error(message);
}
std::vector<char> read(const std::filesystem::path& path) {
    std::ifstream file(path, std::ios::binary);
    require(file.good(), "missing DXBC: " + path.string());
    return {std::istreambuf_iterator<char>(file), {}};
}
Com<ID3DBlob> compile(const std::string& code, const char* profile) {
    Com<ID3DBlob> blob, errors;
    const auto result = D3DCompile(code.data(), code.size(), "first-party-fixture", nullptr, nullptr,
                                   "main", profile, D3DCOMPILE_ENABLE_STRICTNESS, 0, blob.put(), errors.put());
    if (FAILED(result) && errors.p) std::cerr << static_cast<const char*>(errors->GetBufferPointer());
    check(result, "D3DCompile fixture");
    return blob;
}
struct Constant {
    std::string name;
    UINT slot = 0;
    std::vector<unsigned char> data;
    Com<ID3D11Buffer> buffer;
};
struct Shader {
    std::vector<char> dxbc;
    Com<ID3D11VertexShader> shader;
    Com<ID3D11ShaderReflection> reflection;
    std::vector<Constant> constants;
    Shader(ID3D11Device* device, const std::filesystem::path& path) : dxbc(read(path)) {
        check(device->CreateVertexShader(dxbc.data(), dxbc.size(), nullptr, shader.put()), "CreateVertexShader");
        check(D3DReflect(dxbc.data(), dxbc.size(), IID_ID3D11ShaderReflection,
                         reinterpret_cast<void**>(reflection.put())), "D3DReflect");
        D3D11_SHADER_DESC desc{};
        check(reflection->GetDesc(&desc), "reflection desc");
        for (UINT i = 0; i < desc.ConstantBuffers; ++i) {
            D3D11_SHADER_BUFFER_DESC buffer_desc{};
            check(reflection->GetConstantBufferByIndex(i)->GetDesc(&buffer_desc), "constant desc");
            D3D11_SHADER_INPUT_BIND_DESC binding{};
            check(reflection->GetResourceBindingDescByName(buffer_desc.Name, &binding), "binding desc");
            Constant item;
            item.name = buffer_desc.Name;
            item.slot = binding.BindPoint;
            if (item.name == "RSFFoliagePrevious")
                require(buffer_desc.Size == 5056 && item.slot == 13, "reflected foliage ABI size/slot mismatch");
            item.data.resize((buffer_desc.Size + 15) & ~15u);
            D3D11_BUFFER_DESC bd{};
            bd.ByteWidth = static_cast<UINT>(item.data.size());
            bd.BindFlags = D3D11_BIND_CONSTANT_BUFFER;
            check(device->CreateBuffer(&bd, nullptr, item.buffer.put()), "constant buffer");
            constants.push_back(std::move(item));
        }
    }
    void set(const char* name, const void* values, size_t bytes) {
        for (auto& cb : constants) {
            auto* variable = reflection->GetConstantBufferByName(cb.name.c_str())->GetVariableByName(name);
            D3D11_SHADER_VARIABLE_DESC desc{};
            if (SUCCEEDED(variable->GetDesc(&desc))) {
                require(bytes <= desc.Size, std::string("oversized variable ") + name);
                std::memcpy(cb.data.data() + desc.StartOffset, values, bytes);
                return;
            }
        }
    }
    void vector(const char* name, std::array<float, 4> values) { set(name, values.data(), sizeof(values)); }
    void matrix(const char* name, float scale = 1, float tx = 0, float ty = 0) {
        for (auto& cb : constants) {
            auto* variable = reflection->GetConstantBufferByName(cb.name.c_str())->GetVariableByName(name);
            D3D11_SHADER_VARIABLE_DESC desc{};
            if (FAILED(variable->GetDesc(&desc))) continue;
            D3D11_SHADER_TYPE_DESC type{};
            check(variable->GetType()->GetDesc(&type), "matrix type");
            auto* target = cb.data.data() + desc.StartOffset;
            for (UINT r = 0; r < type.Rows; ++r) {
                for (UINT c = 0; c < type.Columns; ++c) {
                    const UINT index = type.Class == D3D_SVC_MATRIX_ROWS ? r * 4 + c : c * 4 + r;
                    const float value = c == 3 && r < 2 ? (r == 0 ? tx : ty) :
                                        r == c ? (r == 3 ? 1.0f : scale) : 0.0f;
                    std::memcpy(target + index * sizeof(float), &value, sizeof(float));
                }
            }
            return;
        }
    }
    void bind(ID3D11DeviceContext* context) {
        std::array<ID3D11Buffer*, 14> empty{};
        context->VSSetConstantBuffers(0, static_cast<UINT>(empty.size()), empty.data());
        for (auto& cb : constants) {
            context->UpdateSubresource(cb.buffer.p, 0, nullptr, cb.data.data(), 0, 0);
            context->VSSetConstantBuffers(cb.slot, 1, &cb.buffer.p);
        }
        context->VSSetShader(shader.p, nullptr, 0);
    }
};
std::vector<float> rows(bool prior) {
    std::vector<float> result(61 * 16);
    for (int group = 0; group < 61; ++group) {
        const size_t start = static_cast<size_t>(group) * 16;
        const float scale = prior ? 0.5f : 1.0f;
        result[start] = result[start + 5] = result[start + 10] = scale;
        result[start + 3] = (prior ? -0.3f : 0.1f) + static_cast<float>(group) * 0.01f;
        result[start + 12] = result[start + 15] = 0.5f;
    }
    return result;
}
void setup(Shader& shader, bool prior) {
    for (auto name : {"m_V", "m_WV", "m_xform_v"}) shader.matrix(name);
    for (auto name : {"m_VP", "m_WVP"}) shader.matrix(name, 1, prior ? 0.05f : 0, prior ? -0.04f : 0);
    shader.matrix("m_xform", prior ? 0.75f : 1.0f, prior ? -0.2f : 0.1f, prior ? 0.1f : 0);
    shader.vector("consts", {prior ? 0.5f : 1.0f, 1, 1, 1});
    shader.vector("wave", {0, 0, 0, prior ? 0.0f : 0.5f});
    for (auto name : {"wind", "dir2D"}) shader.vector(name, {prior ? 0.5f : 0.25f, 0, prior ? 0.25f : 0.125f, 0});
    shader.vector("c_scale", {1, 1, 1, 1});
    const auto data = rows(prior);
    shader.set("array", data.data(), data.size() * sizeof(float));
}
void history(Shader& shader) {
    shader.matrix("rsf_previous_clip_from_world", 1, 0.05f, -0.04f);
    shader.matrix("rsf_previous_tree_xform", 0.75f, -0.2f, 0.1f);
    shader.vector("rsf_previous_consts", {0.5f, 1, 1, 1});
    shader.vector("rsf_previous_wave", {0, 0, 0, 0});
    shader.vector("rsf_previous_wind", {0.5f, 0, 0.25f, 0});
    const std::array<UINT, 4> flags{1, 61, 1, 0};
    shader.set("rsf_foliage_history", flags.data(), sizeof(flags));
    auto previous = rows(true);
    shader.set("rsf_previous_rows", previous.data(), previous.size() * sizeof(float));
    std::array<float, 61 * 4> state{};
    for (size_t group = 0; group < 61; ++group) {
        state[group * 4] = 1;
        state[group * 4 + 1] = 0.5f;
        state[group * 4 + 2] = 0.75f;
    }
    shader.set("rsf_previous_item_state", state.data(), sizeof(state));
}
struct Vertex {
    std::array<float, 4> position{0.2f, 0.4f, 0.5f, 1};
    std::array<float, 4> normal{0, 1, 0, 1};
    std::array<float, 3> tangent{1, 0, 0};
    std::array<float, 3> binormal{0, 0, 1};
    std::array<int, 4> misc{0, 0, 1, 0};
};
struct Capture { std::array<float, 4> current{}, previous{}; float valid = 0; };
struct Device {
    Com<ID3D11Device> device;
    Com<ID3D11DeviceContext> context;
    Device() {
        D3D_FEATURE_LEVEL level{};
        auto result = D3D11CreateDevice(nullptr, D3D_DRIVER_TYPE_HARDWARE, nullptr, 0, nullptr, 0,
                                        D3D11_SDK_VERSION, device.put(), &level, context.put());
        bool hardware = SUCCEEDED(result);
        if (!hardware) result = D3D11CreateDevice(nullptr, D3D_DRIVER_TYPE_WARP, nullptr, 0, nullptr, 0,
                                                  D3D11_SDK_VERSION, device.put(), &level, context.put());
        check(result, "D3D11CreateDevice");
        require(level >= D3D_FEATURE_LEVEL_11_0, "feature level 11 required");
        std::cout << "device=" << (hardware ? "hardware" : "WARP") << " feature_level=" << level << '\n';
        Com<IDXGIDevice> dxgi_device;
        Com<IDXGIAdapter> adapter;
        check(device->QueryInterface(IID_IDXGIDevice, reinterpret_cast<void**>(dxgi_device.put())), "DXGI device");
        check(dxgi_device->GetAdapter(adapter.put()), "DXGI adapter");
        DXGI_ADAPTER_DESC desc{};
        check(adapter->GetDesc(&desc), "adapter desc");
        std::array<char, 512> name{};
        require(WideCharToMultiByte(CP_UTF8, 0, desc.Description, -1, name.data(), static_cast<int>(name.size()), nullptr, nullptr) != 0, "adapter name UTF8");
        std::cout << "adapter=" << name.data() << " vendor=" << desc.VendorId << " device=" << desc.DeviceId << '\n';
    }
    std::array<std::array<float, 4>, 3> raster(Shader& shader, const std::filesystem::path& pixel_path,
                                                  bool detail, bool diagnostic = false) {
        // Real asset VS and real core-transformed PS, no geometry shader.
        auto pixel = read(pixel_path);
        Com<ID3D11PixelShader> ps;
        if (!diagnostic) {
            check(device->CreatePixelShader(pixel.data(), pixel.size(), nullptr, ps.put()), "asset pixel shader");
        } else {
            D3D11_SHADER_DESC signature{};
            check(shader.reflection->GetDesc(&signature), "diagnostic signature");
            std::string code = "struct T {", previous_field, valid_field;
            for (UINT i = 0; i < signature.OutputParameters; ++i) {
                D3D11_SIGNATURE_PARAMETER_DESC parameter{};
                check(shader.reflection->GetOutputParameterDesc(i, &parameter), "diagnostic field");
                UINT width = 0;
                for (UINT bits = parameter.Mask; bits; bits >>= 1) ++width;
                const std::string field = "f" + std::to_string(i);
                const bool is_valid = std::string(parameter.SemanticName) == "TEXCOORD" && parameter.SemanticIndex == 13;
                if (is_valid) code += "nointerpolation ";
                code += "float" + std::to_string(width) + " " + field + ":" + parameter.SemanticName + std::to_string(parameter.SemanticIndex) + ";";
                if (std::string(parameter.SemanticName) == "TEXCOORD" && parameter.SemanticIndex == 12) previous_field = field;
                if (is_valid) valid_field = field;
            }
            code += "};struct R {float4 p:SV_Target0;float4 v:SV_Target1;};R main(T i){R o;o.p=i." + previous_field + ";o.v=float4(i." + valid_field + ",0,0,0);return o;}";
            auto blob = compile(code, "ps_5_0");
            check(device->CreatePixelShader(blob->GetBufferPointer(), blob->GetBufferSize(), nullptr, ps.put()), "diagnostic pixel shader");
        }
        std::array<Vertex, 3> vertices{};
        vertices[0].position = {-0.8f, -0.7f, 0.5f, 1};
        vertices[1].position = {0, 0.8f, 0.5f, 1};
        vertices[2].position = {0.8f, -0.7f, 0.5f, 1};
        for (auto& vertex : vertices) vertex.misc[3] = detail ? 4 : 0;
        std::vector<D3D11_INPUT_ELEMENT_DESC> elements{
            {"POSITION", 0, DXGI_FORMAT_R32G32B32A32_FLOAT, 0, 0, D3D11_INPUT_PER_VERTEX_DATA, 0}};
        if (!detail) {
            elements.push_back({"NORMAL", 0, DXGI_FORMAT_R32G32B32A32_FLOAT, 0, 16, D3D11_INPUT_PER_VERTEX_DATA, 0});
            elements.push_back({"TANGENT", 0, DXGI_FORMAT_R32G32B32_FLOAT, 0, 32, D3D11_INPUT_PER_VERTEX_DATA, 0});
            elements.push_back({"BINORMAL", 0, DXGI_FORMAT_R32G32B32_FLOAT, 0, 44, D3D11_INPUT_PER_VERTEX_DATA, 0});
        }
        elements.push_back({"TEXCOORD", 0, DXGI_FORMAT_R32G32B32A32_SINT, 0, 56, D3D11_INPUT_PER_VERTEX_DATA, 0});
        Com<ID3D11InputLayout> layout;
        check(device->CreateInputLayout(elements.data(), static_cast<UINT>(elements.size()), shader.dxbc.data(),
                                        shader.dxbc.size(), layout.put()), "raster layout");
        Com<ID3D11Buffer> vb;
        D3D11_BUFFER_DESC bd{}; bd.ByteWidth = sizeof(vertices); bd.BindFlags = D3D11_BIND_VERTEX_BUFFER;
        D3D11_SUBRESOURCE_DATA initial{}; initial.pSysMem = vertices.data();
        check(device->CreateBuffer(&bd, &initial, vb.put()), "raster vertex buffer");
        const UINT stride = sizeof(Vertex), offset = 0;
        context->IASetVertexBuffers(0, 1, &vb.p, &stride, &offset);
        context->IASetInputLayout(layout.p);
        context->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
        context->GSSetShader(nullptr, nullptr, 0);
        context->PSSetShader(ps.p, nullptr, 0);
        shader.bind(context.p);
        D3D11_VIEWPORT viewport{0, 0, 16, 16, 0, 1};
        context->RSSetViewports(1, &viewport);
        Com<ID3D11RasterizerState> rasterizer;
        D3D11_RASTERIZER_DESC rs{}; rs.FillMode = D3D11_FILL_SOLID; rs.CullMode = D3D11_CULL_NONE; rs.DepthClipEnable = TRUE;
        check(device->CreateRasterizerState(&rs, rasterizer.put()), "rasterizer");
        context->RSSetState(rasterizer.p);
        Com<ID3D11DepthStencilState> depth;
        D3D11_DEPTH_STENCIL_DESC ds{}; ds.DepthFunc = D3D11_COMPARISON_ALWAYS;
        check(device->CreateDepthStencilState(&ds, depth.put()), "disabled depth");
        context->OMSetDepthStencilState(depth.p, 0);
        Com<ID3D11Buffer> ps_constants;
        bd.ByteWidth = 48; bd.BindFlags = D3D11_BIND_CONSTANT_BUFFER;
        const std::array<float, 12> ps_data{0, 0, 1.0f / 16, 1.0f / 16, 0, 0, 0, 0, 1, 0, 0, 0};
        initial.pSysMem = ps_data.data();
        check(device->CreateBuffer(&bd, &initial, ps_constants.put()), "PS b13");
        context->PSSetConstantBuffers(13, 1, &ps_constants.p);
        Com<ID3D11Texture2D> white;
        Com<ID3D11ShaderResourceView> white_view;
        D3D11_TEXTURE2D_DESC td{}; td.Width = td.Height = td.MipLevels = td.ArraySize = td.SampleDesc.Count = 1;
        td.Format = DXGI_FORMAT_R8G8B8A8_UNORM; td.BindFlags = D3D11_BIND_SHADER_RESOURCE;
        const UINT texel = 0xffffffffu; initial.pSysMem = &texel; initial.SysMemPitch = 4;
        check(device->CreateTexture2D(&td, &initial, white.put()), "white fixture texture");
        check(device->CreateShaderResourceView(white.p, nullptr, white_view.put()), "white fixture SRV");
        std::array<ID3D11ShaderResourceView*, 16> views{}; views.fill(white_view.p);
        context->PSSetShaderResources(0, static_cast<UINT>(views.size()), views.data());
        Com<ID3D11SamplerState> sampler;
        D3D11_SAMPLER_DESC sd{}; sd.Filter = D3D11_FILTER_MIN_MAG_MIP_POINT;
        sd.AddressU = sd.AddressV = sd.AddressW = D3D11_TEXTURE_ADDRESS_CLAMP; sd.MaxLOD = D3D11_FLOAT32_MAX;
        check(device->CreateSamplerState(&sd, sampler.put()), "fixture sampler");
        std::array<ID3D11SamplerState*, 16> samplers{}; samplers.fill(sampler.p);
        context->PSSetSamplers(0, static_cast<UINT>(samplers.size()), samplers.data());
        std::array<Com<ID3D11Texture2D>, 3> targets;
        std::array<Com<ID3D11RenderTargetView>, 3> rtvs;
        std::array<ID3D11RenderTargetView*, 3> bindings{};
        td.Width = td.Height = 16; td.BindFlags = D3D11_BIND_RENDER_TARGET;
        for (size_t i = 0; i < targets.size(); ++i) {
            td.Format = i == 2 ? DXGI_FORMAT_R16G16B16A16_FLOAT : DXGI_FORMAT_R32G32B32A32_FLOAT;
            check(device->CreateTexture2D(&td, nullptr, targets[i].put()), "fixture MRT");
            check(device->CreateRenderTargetView(targets[i].p, nullptr, rtvs[i].put()), "fixture RTV");
            const float zero[4]{};
            context->ClearRenderTargetView(rtvs[i].p, zero);
            bindings[i] = rtvs[i].p;
        }
        context->OMSetRenderTargets(3, bindings.data(), nullptr);
        context->Draw(3, 0);
        context->OMSetRenderTargets(0, nullptr, nullptr);
        std::array<std::array<float, 4>, 3> result{};
        for (size_t i = 0; i < targets.size(); ++i) {
            Com<ID3D11Texture2D> staging;
            targets[i]->GetDesc(&td); td.BindFlags = 0; td.Usage = D3D11_USAGE_STAGING; td.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
            check(device->CreateTexture2D(&td, nullptr, staging.put()), "MRT readback texture");
            context->CopyResource(staging.p, targets[i].p);
            D3D11_MAPPED_SUBRESOURCE mapped{};
            check(context->Map(staging.p, 0, D3D11_MAP_READ, 0, &mapped), "MRT map");
            const auto* pixel_data = static_cast<const unsigned char*>(mapped.pData) + 8 * mapped.RowPitch + 8 * (i == 2 ? 8 : 16);
            if (i < 2) std::memcpy(result[i].data(), pixel_data, 16);
            else for (size_t lane = 0; lane < 4; ++lane) {
                unsigned short half = 0;
                std::memcpy(&half, pixel_data + lane * 2, 2);
                const int exponent = (half >> 10) & 31;
                const float fraction = static_cast<float>(half & 1023);
                float value = exponent ? std::ldexp(1.0f + fraction / 1024, exponent - 15) : std::ldexp(fraction, -24);
                if (exponent == 31) value = std::nanf("");
                result[i][lane] = half & 0x8000 ? -value : value;
            }
            context->Unmap(staging.p, 0);
        }
        return result;
    }
    Capture draw(Shader& shader, bool changed, bool detail, int row = 0) {
        Vertex vertex;
        vertex.misc[3] = row;
        std::vector<D3D11_INPUT_ELEMENT_DESC> elements{
            {"POSITION", 0, DXGI_FORMAT_R32G32B32A32_FLOAT, 0, 0, D3D11_INPUT_PER_VERTEX_DATA, 0}};
        if (!detail) {
            elements.push_back({"NORMAL", 0, DXGI_FORMAT_R32G32B32A32_FLOAT, 0, 16, D3D11_INPUT_PER_VERTEX_DATA, 0});
            elements.push_back({"TANGENT", 0, DXGI_FORMAT_R32G32B32_FLOAT, 0, 32, D3D11_INPUT_PER_VERTEX_DATA, 0});
            elements.push_back({"BINORMAL", 0, DXGI_FORMAT_R32G32B32_FLOAT, 0, 44, D3D11_INPUT_PER_VERTEX_DATA, 0});
        }
        elements.push_back({"TEXCOORD", 0, DXGI_FORMAT_R32G32B32A32_SINT, 0, 56, D3D11_INPUT_PER_VERTEX_DATA, 0});
        Com<ID3D11InputLayout> layout;
        check(device->CreateInputLayout(elements.data(), static_cast<UINT>(elements.size()), shader.dxbc.data(),
                                        shader.dxbc.size(), layout.put()), "input layout");
        Com<ID3D11Buffer> vb, so, readback;
        D3D11_BUFFER_DESC bd{};
        bd.ByteWidth = sizeof(vertex); bd.BindFlags = D3D11_BIND_VERTEX_BUFFER;
        D3D11_SUBRESOURCE_DATA initial{}; initial.pSysMem = &vertex;
        check(device->CreateBuffer(&bd, &initial, vb.put()), "vertex buffer");
        bd.ByteWidth = sizeof(Capture); bd.BindFlags = D3D11_BIND_STREAM_OUTPUT;
        check(device->CreateBuffer(&bd, nullptr, so.put()), "SO buffer");
        bd.BindFlags = 0; bd.Usage = D3D11_USAGE_STAGING; bd.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
        check(device->CreateBuffer(&bd, nullptr, readback.put()), "SO readback");
        UINT stride = sizeof(vertex), offset = 0;
        context->IASetInputLayout(layout.p);
        context->IASetVertexBuffers(0, 1, &vb.p, &stride, &offset);
        context->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_POINTLIST);
        D3D11_SHADER_DESC signature{};
        check(shader.reflection->GetDesc(&signature), "GS signature desc");
        std::string code = "struct T {";
        std::string current_field, previous_field, valid_field;
        for (UINT i = 0; i < signature.OutputParameters; ++i) {
            D3D11_SIGNATURE_PARAMETER_DESC parameter{};
            check(shader.reflection->GetOutputParameterDesc(i, &parameter), "GS signature parameter");
            const std::string field = "field" + std::to_string(i);
            UINT width = 0;
            for (UINT bits = parameter.Mask; bits; bits >>= 1) ++width;
            if (parameter.SemanticIndex == 13 && std::string(parameter.SemanticName) == "TEXCOORD") code += "nointerpolation ";
            code += "float" + std::to_string(width) + " " + field + ":" + parameter.SemanticName + std::to_string(parameter.SemanticIndex) + ";";
            if (parameter.SystemValueType == D3D_NAME_POSITION) current_field = field;
            if (std::string(parameter.SemanticName) == "TEXCOORD" && parameter.SemanticIndex == 12) previous_field = field;
            if (std::string(parameter.SemanticName) == "TEXCOORD" && parameter.SemanticIndex == 13) valid_field = field;
        }
        code += "}; struct U {float4 c:TEXCOORD10;";
        if (changed) code += "float4 p:TEXCOORD12; nointerpolation float v:TEXCOORD13;";
        code += "};[maxvertexcount(1)] void main(point T i[1],inout PointStream<U> s){U o;o.c=i[0]." + current_field + ";";
        if (changed) code += "o.p=i[0]." + previous_field + ";o.v=i[0]." + valid_field + ";";
        code += "s.Append(o);}";
        auto gs_blob = compile(code, "gs_5_0");
        std::array<D3D11_SO_DECLARATION_ENTRY, 3> declaration{{
            {0, "TEXCOORD", 10, 0, 4, 0}, {0, "TEXCOORD", 12, 0, 4, 0}, {0, "TEXCOORD", 13, 0, 1, 0}}};
        const UINT so_stride = changed ? sizeof(Capture) : 16;
        Com<ID3D11GeometryShader> gs;
        check(device->CreateGeometryShaderWithStreamOutput(gs_blob->GetBufferPointer(), gs_blob->GetBufferSize(),
              declaration.data(), changed ? 3 : 1, &so_stride, 1, D3D11_SO_NO_RASTERIZED_STREAM, nullptr, gs.put()), "signature matched SO GS");
        context->GSSetShader(gs.p, nullptr, 0);
        context->SOSetTargets(1, &so.p, &offset);
        shader.bind(context.p);
        context->Draw(1, 0);
        context->SOSetTargets(0, nullptr, nullptr);
        context->CopyResource(readback.p, so.p);
        D3D11_MAPPED_SUBRESOURCE mapped{};
        check(context->Map(readback.p, 0, D3D11_MAP_READ, 0, &mapped), "SO map");
        Capture result{};
        std::memcpy(&result, mapped.pData, changed ? sizeof(result) : 16);
        context->Unmap(readback.p, 0);
        return result;
    }
};
void equal(const std::array<float, 4>& a, const std::array<float, 4>& b, const std::string& label) {
    for (size_t i = 0; i < 4; ++i) require(std::isfinite(a[i]) && std::abs(a[i] - b[i]) < 0.00001f,
                                           label + " component " + std::to_string(i) + ": " +
                                           std::to_string(a[i]) + " != " + std::to_string(b[i]));
}
} // namespace
int main(int argc, char** argv) {
    try {
        require(argc == 2, "fixture requires DXBC folder");
        const std::filesystem::path folder(argv[1]);
        Device gpu;
        for (const std::string tag : {"tree", "tree_bump", "tree_disabled", "tree_still", "tree_still_bump", "detail_wave", "detail_still"}) {
            const bool detail = tag.starts_with("detail");
            Shader current(gpu.device.p, folder / (tag + "-original.dxbc"));
            Shader previous(gpu.device.p, folder / (tag + "-original.dxbc"));
            Shader changed(gpu.device.p, folder / (tag + "-changed.dxbc"));
            setup(current, false); setup(previous, true); setup(changed, false); history(changed);
            for (int row : detail ? std::vector<int>{4, 240} : std::vector<int>{0}) {
                const auto native = gpu.draw(current, false, detail, row);
                const auto reference = gpu.draw(previous, false, detail, row);
                const auto result = gpu.draw(changed, true, detail, row);
                equal(result.current, native.current, tag + " unchanged current");
                equal(result.previous, reference.current, tag + " previous deformation reference");
                require(result.valid == 1, tag + " valid history unexpectedly rejected");
                const float dx = (result.current[0] / result.current[3] - result.previous[0] / result.previous[3]) * 0.5f;
                const float dy = (result.previous[1] / result.previous[3] - result.current[1] / result.current[3]) * 0.5f;
                require(std::abs(dx) + std::abs(dy) > 0.0001f, "fixture must generate nonzero deformation motion");
                std::cout << tag << " row=" << row << " current=(" << result.current[0] << ',' << result.current[1]
                          << ") previous=(" << result.previous[0] << ',' << result.previous[1]
                          << ") UV_delta=(" << dx << ',' << dy << ") previous-state GPU reference matches\n";
            }
            const std::string pixel_kind = tag.find("bump") != std::string::npos ? "bump" : "flat";
            const auto pixel_original = folder / ("pixel-" + pixel_kind + "-original.dxbc");
            const auto pixel_changed = folder / ("pixel-" + pixel_kind + "-changed.dxbc");
            const auto native_targets = gpu.raster(current, pixel_original, detail);
            const auto targets = gpu.raster(changed, pixel_changed, detail);
            const auto diagnostic = gpu.raster(changed, pixel_changed, detail, true);
            equal(targets[0], native_targets[0], tag + " unchanged MRT0");
            equal(targets[1], native_targets[1], tag + " unchanged MRT1");
            require(diagnostic[1][0] == 1 && targets[2][2] == 1 && targets[2][3] == 0, tag + " MRT valid coverage");
            const float expected_x = 8.5f / 16 - (diagnostic[0][0] / diagnostic[0][3] * 0.5f + 0.5f);
            const float expected_y = 8.5f / 16 - (diagnostic[0][1] / diagnostic[0][3] * -0.5f + 0.5f);
            require(std::abs(targets[2][0] - expected_x) < 0.00025f && std::abs(targets[2][1] - expected_y) < 0.00025f,
                    tag + " real motion MRT must match interpolated previous clip");
            require(std::abs(targets[2][0]) + std::abs(targets[2][1]) > 0.0001f, tag + " real motion cannot be manufactured zero");
            std::cout << tag << " MRT2 rendered=(" << targets[2][0] << ',' << targets[2][1] << ',' << targets[2][2] << ',' << targets[2][3]
                      << ") RGBA16_FLOAT, original MRT0/1 unchanged\n";
            const std::array<UINT, 4> missing{0, 61, 1, 0};
            changed.set("rsf_foliage_history", missing.data(), sizeof(missing));
            require(gpu.draw(changed, true, detail, detail ? 4 : 0).valid == 0, "missing draw history must invalidate");
            const auto invalid_targets = gpu.raster(changed, pixel_changed, detail);
            equal(invalid_targets[2], {0, 0, 0, 0}, tag + " missing history clears motion coverage");
            equal(invalid_targets[0], native_targets[0], tag + " invalid history must preserve native MRT0");
            history(changed);
            if (detail) {
                std::array<float, 61 * 4> state{};
                changed.set("rsf_previous_item_state", state.data(), sizeof(state));
                require(gpu.draw(changed, true, true, 4).valid == 0, "missing stable item history must invalidate");
                history(changed);
                state[4] = 1; state[5] = 0.5f; state[6] = 0;
                changed.set("rsf_previous_item_state", state.data(), sizeof(state));
                require(gpu.draw(changed, true, true, 4).valid == 0, "missing previous fade must invalidate");
                state[6] = 0.75f; state[5] = std::nanf("");
                changed.set("rsf_previous_item_state", state.data(), sizeof(state));
                require(gpu.draw(changed, true, true, 4).valid == 0, "nonfinite previous scale must invalidate");
                history(changed);
                auto bad_rows = rows(true);
                bad_rows[16] = std::nanf("");
                changed.set("rsf_previous_rows", bad_rows.data(), bad_rows.size() * sizeof(float));
                require(gpu.draw(changed, true, true, 4).valid == 0, "nonfinite previous instance row must invalidate");
                history(changed);
                require(gpu.draw(changed, true, true, 2).valid == 0, "unaligned previous row must invalidate");
                require(gpu.draw(changed, true, true, 244).valid == 0, "out of range previous row must invalidate");
                std::cout << tag << " invalid fade/scale and invalid row indices rejected\n";
            }
            std::cout << tag << " missing history invalidates\n";
        }
        return 0;
    } catch (const std::exception& error) {
        std::cerr << error.what() << '\n';
        return 1;
    }
}

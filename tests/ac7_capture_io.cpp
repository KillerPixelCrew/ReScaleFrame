// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/ac7_motion_capture.h>
#include <rescaleframe/d3d11_observer.h>
#include <rescaleframe/frame_tap.h>
#include <rescaleframe/texture_dump.h>
#include <windows.h>
#include <d3d11.h>
#include <d3dcompiler.h>
#include <wrl/client.h>
#include <cstdio>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <iterator>
#include <string>

using Microsoft::WRL::ComPtr;
namespace {
uint32_t stages = 0;
void shader_seen(void*, void* shader, uint32_t stage, const void* bytes, uint32_t size)
{
    stages |= 1u << stage;
    rsf_ac7_motion_capture_shader(shader, stage, bytes, size);
}
void upload(void*, void* buffer, void* bytes, uint32_t size)
{ rsf_ac7_motion_capture_upload(buffer, bytes, size); }
void layout_seen(void*, void* layout, const rsf_observer_layout_element* elements,
                 uint32_t copied, uint32_t count)
{ rsf_ac7_motion_capture_layout(layout, elements, copied, count); }
std::string read_file(const std::filesystem::path& path)
{
    std::ifstream input(path, std::ios::binary);
    return {std::istreambuf_iterator<char>(input), std::istreambuf_iterator<char>()};
}
}
int main()
{
    bool passed = true;
    auto check = [&](bool condition, const char* what) {
        if (!condition) { std::fprintf(stderr, "%s\n", what); passed = false; }
    };
    const auto directory = std::filesystem::temp_directory_path() /
        ("rsf-motion-capture-" + std::to_string(GetCurrentProcessId()));
    std::filesystem::create_directories(directory);
    rsf_ac7_motion_capture_configure(directory.string().c_str(), nullptr, nullptr);
    check(rsf_ac7_motion_capture_install() == 0, "Wrong executable must refuse the engine detours.");

    rsf_observer_options observer{};
    observer.struct_size = sizeof(observer); observer.abi_version = RSF_OBSERVER_ABI_VERSION;
    observer.on_shader = shader_seen; observer.on_buffer = rsf_ac7_motion_capture_buffer;
    observer.on_layout = layout_seen;
    check(rsf_observer_install(&observer) == RSF_OBSERVER_OK, "Observer must install.");

    WNDCLASSW window_class{}; window_class.lpfnWndProc = DefWindowProcW;
    window_class.hInstance = GetModuleHandleW(nullptr); window_class.lpszClassName = L"RSF_MotionCaptureTest";
    RegisterClassW(&window_class);
    HWND window = CreateWindowW(window_class.lpszClassName, L"", WS_OVERLAPPED, 0, 0, 32, 32,
                                nullptr, nullptr, window_class.hInstance, nullptr);
    DXGI_SWAP_CHAIN_DESC swap{};
    swap.BufferDesc.Width = 16; swap.BufferDesc.Height = 16;
    swap.BufferDesc.Format = DXGI_FORMAT_R8G8B8A8_UNORM; swap.SampleDesc.Count = 1;
    swap.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT; swap.BufferCount = 1;
    swap.OutputWindow = window; swap.Windowed = TRUE;
    ComPtr<ID3D11Device> device; ComPtr<ID3D11DeviceContext> context; ComPtr<IDXGISwapChain> chain;
    if (FAILED(D3D11CreateDeviceAndSwapChain(nullptr, D3D_DRIVER_TYPE_WARP, nullptr, 0, nullptr, 0,
        D3D11_SDK_VERSION, &swap, &chain, &device, nullptr, &context))) return 77;
    auto compile = [&](const char* text, const char* profile) {
        ComPtr<ID3DBlob> code, errors;
        check(SUCCEEDED(D3DCompile(text, std::strlen(text), nullptr, nullptr, nullptr,
            "main", profile, 0, 0, &code, &errors)), "Capture fixture shader must compile.");
        if (errors) std::fprintf(stderr, "%s\n", static_cast<const char*>(errors->GetBufferPointer()));
        return code;
    };
    auto vs_code = compile("float4 main(uint id:SV_VertexID):SV_Position"
        "{return float4(id==2?3:-1,id==1?3:-1,0,1);}", "vs_5_0");
    auto ps_code = compile("cbuffer P:register(b0){float4 colour;} float4 main():SV_Target{return colour;}", "ps_5_0");
    auto cs_code = compile("RWTexture2D<float4> result:register(u0); [numthreads(1,1,1)]"
        "void main(uint3 id:SV_DispatchThreadID){result[id.xy]=float4(.25,.5,.75,1);}", "cs_5_0");
    auto particle_code = compile("float4 main(float4 p:ATTRIBUTE0,float4 old:ATTRIBUTE1):SV_Position"
        "{return p+old*.01;}", "vs_5_0");
    auto gs_code = compile("struct V{float4 p:SV_Position;};[maxvertexcount(3)]"
        "void main(triangle V v[3],inout TriangleStream<V> stream)"
        "{stream.Append(v[0]);stream.Append(v[1]);stream.Append(v[2]);}", "gs_5_0");
    auto hs_code = compile("struct V{float4 p:SV_Position;};struct F{float edge[3]:SV_TessFactor;float inside:SV_InsideTessFactor;};"
        "F patch(InputPatch<V,3> v){F f;f.edge[0]=f.edge[1]=f.edge[2]=f.inside=1;return f;}"
        "[domain(\"tri\")][partitioning(\"integer\")][outputtopology(\"triangle_cw\")]"
        "[outputcontrolpoints(3)][patchconstantfunc(\"patch\")]"
        "V main(InputPatch<V,3> v,uint id:SV_OutputControlPointID){return v[id];}", "hs_5_0");
    auto ds_code = compile("struct V{float4 p:SV_Position;};struct F{float edge[3]:SV_TessFactor;float inside:SV_InsideTessFactor;};"
        "[domain(\"tri\")] V main(F f,float3 uv:SV_DomainLocation,const OutputPatch<V,3> v)"
        "{V o;o.p=v[0].p*uv.x+v[1].p*uv.y+v[2].p*uv.z;return o;}", "ds_5_0");
    if (!passed) return 1;
    const D3D11_INPUT_ELEMENT_DESC particle_elements[]{
        {"ATTRIBUTE",0,DXGI_FORMAT_R32G32B32A32_FLOAT,0,0,D3D11_INPUT_PER_VERTEX_DATA,0},
        {"ATTRIBUTE",1,DXGI_FORMAT_R32G32B32A32_FLOAT,0,16,D3D11_INPUT_PER_VERTEX_DATA,0},
    };
    ComPtr<ID3D11InputLayout> particle_layout;
    check(SUCCEEDED(device->CreateInputLayout(particle_elements, 2, particle_code->GetBufferPointer(),
        particle_code->GetBufferSize(), &particle_layout)), "Capture particle stream layout.");
    const float particle_data[24]{-1,-1,0,1, -1,-1,0,.0001f, 3,-1,0,1, 3,-1,0,.0002f,
                                 -1,3,0,1, -1,3,0,.0003f};
    D3D11_BUFFER_DESC particle_desc{}; particle_desc.ByteWidth = sizeof(particle_data);
    particle_desc.Usage = D3D11_USAGE_IMMUTABLE; particle_desc.BindFlags = D3D11_BIND_VERTEX_BUFFER;
    D3D11_SUBRESOURCE_DATA particle_initial{particle_data,0,0};
    ComPtr<ID3D11Buffer> particle_buffer;
    check(SUCCEEDED(device->CreateBuffer(&particle_desc, &particle_initial, &particle_buffer)), "Create particle geometry history stream.");
    ComPtr<ID3D11VertexShader> particle_vs;
    check(SUCCEEDED(device->CreateVertexShader(particle_code->GetBufferPointer(),particle_code->GetBufferSize(),nullptr,&particle_vs)),"Create captured particle VS.");
    const float previous_data[16]{1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16};
    D3D11_BUFFER_DESC previous_desc{};previous_desc.ByteWidth=sizeof(previous_data);
    previous_desc.Usage=D3D11_USAGE_IMMUTABLE;previous_desc.BindFlags=D3D11_BIND_SHADER_RESOURCE;
    D3D11_SUBRESOURCE_DATA previous_initial{previous_data,0,0};ComPtr<ID3D11Buffer> previous_buffer;
    check(SUCCEEDED(device->CreateBuffer(&previous_desc,&previous_initial,&previous_buffer)),"Create previous-transform capture source.");
    D3D11_SHADER_RESOURCE_VIEW_DESC previous_view_desc{};previous_view_desc.Format=DXGI_FORMAT_R32G32B32A32_FLOAT;
    previous_view_desc.ViewDimension=D3D11_SRV_DIMENSION_BUFFER;previous_view_desc.Buffer.FirstElement=1;previous_view_desc.Buffer.NumElements=2;
    ComPtr<ID3D11ShaderResourceView> previous_view;
    check(SUCCEEDED(device->CreateShaderResourceView(previous_buffer.Get(),&previous_view_desc,&previous_view)),"Create sliced previous-transform view.");
    D3D11_QUERY_DESC predicate_desc{D3D11_QUERY_OCCLUSION_PREDICATE,0};ComPtr<ID3D11Predicate> hidden;
    check(SUCCEEDED(device->CreatePredicate(&predicate_desc,&hidden)),"Create predicated capture fixture.");
    context->Begin(hidden.Get());context->End(hidden.Get());
    D3D11_TEXTURE2D_DESC velocity_depth_desc{};velocity_depth_desc.Width=velocity_depth_desc.Height=16;
    velocity_depth_desc.MipLevels=velocity_depth_desc.ArraySize=1;velocity_depth_desc.SampleDesc.Count=1;
    velocity_depth_desc.Format=DXGI_FORMAT_D32_FLOAT;velocity_depth_desc.BindFlags=D3D11_BIND_DEPTH_STENCIL;
    ComPtr<ID3D11Texture2D> velocity_depth;ComPtr<ID3D11DepthStencilView> velocity_depth_view;
    check(SUCCEEDED(device->CreateTexture2D(&velocity_depth_desc,nullptr,&velocity_depth)) &&
        SUCCEEDED(device->CreateDepthStencilView(velocity_depth.Get(),nullptr,&velocity_depth_view)),"Create velocity-capture depth binding.");
    ComPtr<ID3D11VertexShader> vs; ComPtr<ID3D11PixelShader> ps; ComPtr<ID3D11ComputeShader> cs;
    ComPtr<ID3D11GeometryShader> gs; ComPtr<ID3D11HullShader> hs; ComPtr<ID3D11DomainShader> ds;
    check(SUCCEEDED(device->CreateVertexShader(vs_code->GetBufferPointer(), vs_code->GetBufferSize(), nullptr, &vs)), "Create VS.");
    check(SUCCEEDED(device->CreatePixelShader(ps_code->GetBufferPointer(), ps_code->GetBufferSize(), nullptr, &ps)), "Create PS.");
    check(SUCCEEDED(device->CreateComputeShader(cs_code->GetBufferPointer(), cs_code->GetBufferSize(), nullptr, &cs)), "Create CS.");
    check(SUCCEEDED(device->CreateGeometryShader(gs_code->GetBufferPointer(), gs_code->GetBufferSize(), nullptr, &gs)), "Create GS.");
    check(SUCCEEDED(device->CreateHullShader(hs_code->GetBufferPointer(), hs_code->GetBufferSize(), nullptr, &hs)), "Create HS.");
    check(SUCCEEDED(device->CreateDomainShader(ds_code->GetBufferPointer(), ds_code->GetBufferSize(), nullptr, &ds)), "Create DS.");
    check(stages == 0x3f, "All six shader creation stages must reach the capture.");
    D3D11_TEXTURE2D_DESC colour_desc{};
    colour_desc.Width = colour_desc.Height = 16; colour_desc.MipLevels = colour_desc.ArraySize = 1;
    colour_desc.Format = DXGI_FORMAT_R8G8B8A8_UNORM; colour_desc.SampleDesc.Count = 1;
    colour_desc.BindFlags = D3D11_BIND_RENDER_TARGET | D3D11_BIND_UNORDERED_ACCESS;
    ComPtr<ID3D11Texture2D> target; ComPtr<ID3D11RenderTargetView> rtv; ComPtr<ID3D11UnorderedAccessView> uav;
    check(SUCCEEDED(device->CreateTexture2D(&colour_desc, nullptr, &target)), "Create colour.");
    check(SUCCEEDED(device->CreateRenderTargetView(target.Get(), nullptr, &rtv)), "Create RTV.");
    check(SUCCEEDED(device->CreateUnorderedAccessView(target.Get(), nullptr, &uav)), "Create UAV.");
    D3D11_BUFFER_DESC cb_desc{}; cb_desc.ByteWidth = 16; cb_desc.BindFlags = D3D11_BIND_CONSTANT_BUFFER;
    float values[4] = {.25f,.5f,.75f,1};
    D3D11_SUBRESOURCE_DATA initial{}; initial.pSysMem = values;
    ComPtr<ID3D11Buffer> cb;
    check(SUCCEEDED(device->CreateBuffer(&cb_desc, &initial, &cb)), "Create initial CB.");
    rsf_frame_tap_options tap{}; tap.struct_size = sizeof(tap); tap.abi_version = RSF_FRAME_TAP_ABI_VERSION;
    check(rsf_frame_tap_install(context.Get(), &tap) == RSF_FRAME_TAP_OK, "Install draw/compute tap.");
    rsf_frame_tap_set_constant_watch(0, upload, nullptr);
    rsf_ac7_motion_capture_request();
    std::string capture_directory;
    for (uint32_t interval = 0; interval <= 60; ++interval) {
        char prefix[640]{};
        if (rsf_ac7_motion_capture_present(chain.Get(), prefix, sizeof(prefix)) && capture_directory.empty())
            capture_directory = std::filesystem::path(prefix).parent_path().string();
        ID3D11RenderTargetView* rt = rtv.Get(); context->OMSetRenderTargets(1, &rt, nullptr);
        context->VSSetShader(vs.Get(), nullptr, 0); context->PSSetShader(ps.Get(), nullptr, 0);
        ID3D11Buffer* constant = cb.Get(); context->PSSetConstantBuffers(0, 1, &constant);
        context->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
        D3D11_VIEWPORT viewport{0,0,16,16,0,1}; context->RSSetViewports(1, &viewport);
        context->Draw(3, 0);
        if (interval == 0) {
            rsf_game_render_pass pass{}; pass.struct_size = sizeof(pass);
            pass.role = RSF_GAME_RENDER_TRANSLUCENCY; pass.scope_id = 17;
            rsf_ac7_motion_capture_native_pass(&pass, 1);
            rsf_frame_tap_geometry geometry{}; geometry.context = context.Get();
            geometry.input_layout = particle_layout.Get(); geometry.vertex_buffers[0] = particle_buffer.Get();
            geometry.vertex_buffers[31]=particle_buffer.Get(); // Bound stale slot absent from the layout.
            geometry.strides[0] = 32; geometry.count = 3;
            const rsf_observer_layout_element generic_layout[]{ {0,DXGI_FORMAT_R32G32B32A32_FLOAT,0,0,0} };
            rsf_ac7_motion_capture_layout(particle_layout.Get(),generic_layout,1,1);
            for (uint32_t i=0;i<12;++i) rsf_ac7_motion_capture_geometry(&geometry);
            const rsf_observer_layout_element sprite_layout[]{
                {0,DXGI_FORMAT_R32G32B32A32_FLOAT,0,0,0},{1,DXGI_FORMAT_R32G32B32A32_FLOAT,0,16,0}};
            rsf_ac7_motion_capture_layout(particle_layout.Get(),sprite_layout,2,2);
            geometry.vertex_shader=particle_vs.Get();
            rsf_ac7_motion_capture_geometry(&geometry);
            const rsf_observer_layout_element mesh_layout[]{
                {8,DXGI_FORMAT_R32G32B32A32_FLOAT,0,0,1},{9,DXGI_FORMAT_R32G32B32A32_FLOAT,0,16,1},
                {10,DXGI_FORMAT_R32G32B32A32_FLOAT,0,32,1}};
            rsf_ac7_motion_capture_layout(particle_layout.Get(),mesh_layout,3,3);
            ID3D11ShaderResourceView* previous=previous_view.Get();context->VSSetShaderResources(127,1,&previous);
            context->SetPredication(hidden.Get(),FALSE);
            rsf_ac7_motion_capture_geometry(&geometry);
            ComPtr<ID3D11Predicate> restored;BOOL predicate_value=TRUE;context->GetPredication(&restored,&predicate_value);
            check(restored.Get()==hidden.Get() && !predicate_value,"Geometry capture must restore native predication.");
            context->SetPredication(nullptr,FALSE);
            pass.role=0;rsf_ac7_motion_capture_native_pass(&pass,1);
            geometry.format=DXGI_FORMAT_R16G16_UNORM;geometry.depth_view=velocity_depth_view.Get();
            context->VSSetShaderResources(0,1,&previous);context->VSSetShaderResources(1,1,&previous);
            rsf_ac7_motion_capture_geometry(&geometry);
            rsf_ac7_motion_capture_native_pass(nullptr, 0);
        }
        context->OMSetRenderTargets(0, nullptr, nullptr);
        ID3D11UnorderedAccessView* output = uav.Get(); context->CSSetUnorderedAccessViews(0, 1, &output, nullptr);
        context->CSSetShader(cs.Get(), nullptr, 0); context->Dispatch(16, 16, 1);
        output = nullptr; context->CSSetUnorderedAccessViews(0, 1, &output, nullptr);
    }
    const std::filesystem::path captured = capture_directory;
    const auto summary = read_file(captured / "session.json");
    const auto draws = read_file(captured / "draws.jsonl");
    check(summary.find("\"files_written\":true") != std::string::npos, "Session must finish and write successfully.");
    check(draws.find("\"kind\":\"draw\"") != std::string::npos &&
          draws.find("\"kind\":\"dispatch\"") != std::string::npos, "Draw and compute records must both exist.");
    check(draws.find("\"blob\":0") == std::string::npos, "Initial CB contents must be captured.");
    check(read_file(captured / "cb_1.bin").size() == 16, "Exact CB bytes must be saved.");
    check(draws.find("\"kind\":\"geometry\"") != std::string::npos &&
          draws.find("\"attribute\":1") != std::string::npos &&
          draws.find("\"sample_bytes\":96") != std::string::npos,
          "Capture must retain actual particle attributes and bounded vertex bytes.");
    check(draws.find("\"geometry_bucket\":1")!=std::string::npos && draws.find("\"geometry_bucket\":2")!=std::string::npos,
        "Early generic geometry must not consume the sprite/mesh capture budgets.");
    check(draws.find("\"slot\":31")==std::string::npos,"Unused IA slots must not consume geometry readbacks.");
    check(draws.find("\"geometry_bucket\":4")!=std::string::npos && draws.find("\"vs_buffers\":[{\"slot\":0")!=std::string::npos,
        "Native velocity candidates must retain their bone-buffer source views outside translucency scope.");
    const auto previous_record=draws.find("\"vs_buffers\":[{\"slot\":127");
    check(previous_record!=std::string::npos,"Mesh-particle previous-transform views must be sampled.");
    if (previous_record!=std::string::npos) {
        const auto blob_start=draws.find("\"blob\":",previous_record);
        const auto blob=std::stoul(draws.substr(blob_start+7));
        const auto data=read_file(captured/("cb_"+std::to_string(blob)+".bin"));
        check(data.size()==32 && std::memcmp(data.data(),previous_data+4,32)==0,
            "Capture must copy the SRV's actual slice even while native drawing is predicated out.");
    }

    rsf_texture_dump_options dump{}; dump.struct_size = sizeof(dump); dump.abi_version = RSF_TEXTURE_DUMP_ABI_VERSION;
    const std::string raw_prefix = (directory / "colour").string(); dump.output_prefix_utf8 = raw_prefix.c_str();
    check(rsf_dump_texture_bytes(device.Get(), context.Get(), target.Get(), &dump) == RSF_TEXTURE_OK, "Lossless colour dump.");
    const auto colour = read_file(raw_prefix + ".bin");
    check(colour.size() == 16 * 16 * 4 && uint8_t(colour[0]) == 64 && uint8_t(colour[1]) == 128 &&
          uint8_t(colour[2]) == 191 && uint8_t(colour[3]) == 255, "Capture must preserve rendered pixels.");
    D3D11_TEXTURE2D_DESC present_desc = colour_desc;
    present_desc.Format = DXGI_FORMAT_R10G10B10A2_UNORM;
    present_desc.BindFlags = D3D11_BIND_RENDER_TARGET;
    ComPtr<ID3D11Texture2D> present; ComPtr<ID3D11RenderTargetView> present_view;
    check(SUCCEEDED(device->CreateTexture2D(&present_desc, nullptr, &present)) &&
          SUCCEEDED(device->CreateRenderTargetView(present.Get(), nullptr, &present_view)), "Create 10-bit presented image.");
    context->ClearRenderTargetView(present_view.Get(), values);
    const std::string present_prefix = (directory / "present").string(); dump.output_prefix_utf8 = present_prefix.c_str();
    check(rsf_dump_texture_bytes(device.Get(), context.Get(), present.Get(), &dump) == RSF_TEXTURE_OK, "Lossless 10-bit final capture.");
    rsf_texture_dump_report preview{}; preview.struct_size = sizeof(preview);
    check(rsf_dump_texture(device.Get(), context.Get(), present.Get(), &dump, &preview) == RSF_TEXTURE_OK, "10-bit final preview.");
    const auto present_bytes = read_file(present_prefix + ".bin"); uint32_t packed = 0;
    if (present_bytes.size() >= 4) std::memcpy(&packed, present_bytes.data(), 4);
    check(present_bytes.size() == 1024 && (packed & 1023u) == 256 &&
          ((packed >> 10) & 1023u) == 512 && ((packed >> 20) & 1023u) == 767 &&
          (packed >> 30) == 3, "Final capture must preserve ten-bit channels and alpha.");
    D3D11_TEXTURE2D_DESC depth_desc = colour_desc;
    depth_desc.Format = DXGI_FORMAT_R24G8_TYPELESS;
    depth_desc.BindFlags = D3D11_BIND_DEPTH_STENCIL;
    ComPtr<ID3D11Texture2D> depth; ComPtr<ID3D11DepthStencilView> dsv;
    D3D11_DEPTH_STENCIL_VIEW_DESC depth_view{}; depth_view.Format = DXGI_FORMAT_D24_UNORM_S8_UINT;
    depth_view.ViewDimension = D3D11_DSV_DIMENSION_TEXTURE2D;
    check(SUCCEEDED(device->CreateTexture2D(&depth_desc, nullptr, &depth)) &&
          SUCCEEDED(device->CreateDepthStencilView(depth.Get(), &depth_view, &dsv)), "Create typeless depth.");
    context->ClearDepthStencilView(dsv.Get(), D3D11_CLEAR_DEPTH | D3D11_CLEAR_STENCIL, .25f, 7);
    const std::string depth_prefix = (directory / "depth").string(); dump.output_prefix_utf8 = depth_prefix.c_str();
    check(rsf_dump_texture_bytes(device.Get(), context.Get(), depth.Get(), &dump) == RSF_TEXTURE_OK, "Lossless typeless depth.");
    const auto raw_depth = read_file(depth_prefix + ".bin"); uint32_t depth_bits = 0;
    if (raw_depth.size() >= 4) std::memcpy(&depth_bits, raw_depth.data(), 4);
    check(raw_depth.size() == 1024 && (depth_bits >> 24) == 7 &&
          (depth_bits & 0xffffff) >= 0x3fffff && (depth_bits & 0xffffff) <= 0x400001, "Depth and stencil bits must survive.");
    rsf_ac7_motion_capture_shutdown(); rsf_frame_tap_uninstall(); rsf_observer_uninstall();
    DestroyWindow(window); UnregisterClassW(window_class.lpszClassName, window_class.hInstance);
    std::printf("capture fixture: %s\n", capture_directory.c_str());
    return passed ? 0 : 1;
}

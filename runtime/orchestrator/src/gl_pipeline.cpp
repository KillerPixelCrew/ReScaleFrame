// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/gl_pipeline.h>
#include <rescaleframe/native_sr_d3d12.h>
#include <rescaleframe/native_fg_d3d12.h>
#include <rescaleframe/fg_choice.h>
#include <windows.h>
#include <d3d12.h>
#include <dxgi1_4.h>
#include <wrl/client.h>
#include <array>
#include <string>
#include <memory>
#include <cstdio>
using Microsoft::WRL::ComPtr;
struct rsf_gl_pipeline {
    HWND parent=nullptr,window=nullptr;
    bool shown=false,own_window=false;
    rsf_gl_context* sharing=nullptr;
    rsf_sr12* sr=nullptr;
    std::array<rsf_gl_texture*,6> surfaces{};
    std::array<rsf_gl_texture_info,6> textures{};
    ComPtr<IDXGISwapChain3> chain;
    ID3D12Device* device=nullptr;
    ID3D12CommandQueue* queue=nullptr;
    rsf_gl_pipeline_options options{};
    rsf_gl_pipeline_status status{};
    uint32_t width=0,height=0,render_width=0,render_height=0,generation=0;
    uint64_t session=0;
    std::string root,fsr2,fsr3,fsr4,xess,streamline,interposer,version,project;
    rsf_backend_log_fn log=nullptr;void* user=nullptr;
};
namespace {
constexpr UINT surface_resize=WM_APP+0x525,surface_destroy=WM_APP+0x526;
LRESULT CALLBACK surface_window(HWND window,UINT message,WPARAM wparam,LPARAM lparam) {
    // GLFW remains the input/focus owner. This child owns only DXGI presentation.
    if(message==WM_NCHITTEST)return HTTRANSPARENT;
    if(message==WM_MOUSEACTIVATE)return MA_NOACTIVATE;
    if(message==WM_ERASEBKGND)return 1;
    if(message==surface_resize)return SetWindowPos(window,nullptr,0,0,static_cast<int>(wparam),static_cast<int>(lparam),SWP_NOACTIVATE|SWP_NOZORDER);
    if(message==surface_destroy)return DestroyWindow(window);
    return DefWindowProcW(window,message,wparam,lparam);
}
HWND create_surface(HWND parent) {
    if(GetWindowThreadProcessId(parent,nullptr)!=GetCurrentThreadId())return nullptr;
    WNDCLASSW type{};type.lpfnWndProc=surface_window;type.hInstance=GetModuleHandleW(nullptr);type.lpszClassName=L"ReScaleFrame.OpenGL.Presentation";
    if(!RegisterClassW(&type)&&GetLastError()!=ERROR_CLASS_ALREADY_EXISTS)return nullptr;
    return CreateWindowExW(WS_EX_NOACTIVATE|WS_EX_TRANSPARENT,type.lpszClassName,L"",WS_CHILD|WS_CLIPSIBLINGS,
        0,0,32,32,parent,nullptr,type.hInstance,nullptr);
}
bool surface_message(HWND window,UINT operation,WPARAM wparam=0,LPARAM lparam=0) {
    DWORD_PTR result=0;
    return SendMessageTimeoutW(window,operation,wparam,lparam,SMTO_BLOCK|SMTO_ABORTIFHUNG,5000,&result)&&result!=0;
}
int accept(void* user,void* window) {return static_cast<rsf_gl_pipeline*>(user)->window==window;}
void log(void* user,const char* message) {auto& self=*static_cast<rsf_gl_pipeline*>(user);if(self.log)self.log(self.user,message);}
void barrier(ID3D12GraphicsCommandList* list,void* pointer,D3D12_RESOURCE_STATES from,D3D12_RESOURCE_STATES to) {
    D3D12_RESOURCE_BARRIER change{};change.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    change.Transition={static_cast<ID3D12Resource*>(pointer),D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,from,to};list->ResourceBarrier(1,&change);
}
void release_surfaces(rsf_gl_pipeline& self) {
    rsf_sr12_destroy(self.sr);self.sr=nullptr;
    for(auto*& texture:self.surfaces) {rsf_gl_texture_destroy(texture);texture=nullptr;}
    self.textures={};
}
rsf_backend_result create_sr(rsf_gl_pipeline& self,uint32_t backend,rsf_sr12** out) {
    rsf_sr12_setup setup{};setup.struct_size=sizeof(setup);setup.abi_version=1;setup.device=self.device;
    setup.backend=backend;setup.quality=self.options.quality;setup.output_width=self.width;setup.output_height=self.height;
    setup.fsr2_directory_utf8=self.fsr2.c_str();setup.fsr3_directory_utf8=self.fsr3.c_str();
    setup.fsr4_directory_utf8=self.fsr4.c_str();setup.xess_directory_utf8=self.xess.c_str();
    setup.streamline_host=rsf_d3d11_present_host();setup.input_sdr=1;setup.log=log;setup.user=&self;
    setup.dlss={sizeof(setup.dlss),RSF_DLSS_ABI_VERSION,self.interposer.c_str(),self.streamline.c_str(),self.root.c_str(),
        0,RSF_DLSS_ENGINE_CUSTOM,self.version.c_str(),self.project.c_str(),1,log,&self};
    return rsf_sr12_create(&setup,out);
}
}
extern "C" void* rsf_gl_window_create(void* parent) {return create_surface(static_cast<HWND>(parent));}
extern "C" int rsf_gl_window_destroy(void* window) {return surface_message(static_cast<HWND>(window),surface_destroy)?1:0;}
extern "C" rsf_backend_result rsf_gl_pipeline_open(const rsf_gl_pipeline_setup* setup,rsf_gl_pipeline** out) try {
    if(!out)return RSF_BACKEND_ERROR_INVALID_ARGUMENT;*out=nullptr;
    if(!setup||setup->struct_size<sizeof(*setup)||!IsWindow(static_cast<HWND>(setup->hwnd))||
        !setup->runtime_directory_utf8||!setup->engine_version_utf8||!setup->project_id_utf8)return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    auto self=std::make_unique<rsf_gl_pipeline>();self->parent=static_cast<HWND>(setup->hwnd);
    self->root=setup->runtime_directory_utf8;self->version=setup->engine_version_utf8;self->project=setup->project_id_utf8;
    self->fsr2=self->fsr3=self->fsr4=self->root+"/fidelityfx";
    self->xess=self->root+"/xess";self->streamline=self->root+"/streamline";self->interposer=self->streamline+"/sl.interposer.dll";
    self->log=setup->log;self->user=setup->user;self->session=GetTickCount64()+1;
    rsf_gl_setup sharing{sizeof(sharing),1,nullptr,nullptr,log,self.get()};
    auto result=rsf_gl_open(&sharing,&self->sharing);if(result!=RSF_BACKEND_OK)return result;
    void* device=nullptr;void* queue=nullptr;rsf_gl_graphics(self->sharing,&device,&queue);
    self->device=static_cast<ID3D12Device*>(device);self->queue=static_cast<ID3D12CommandQueue*>(queue);
    // Once WGL has presented even a loading screen, DXGI creation on that HWND
    // returns E_ACCESSDENIED. A child gives the native chain its own compositor
    // surface while GLFW keeps the original context, events and window lifecycle.
    self->own_window=setup->presentation_hwnd==nullptr;
    self->window=self->own_window?create_surface(self->parent):static_cast<HWND>(setup->presentation_hwnd);
    if(self->window&&(GetParent(self->window)!=self->parent||GetWindowThreadProcessId(self->window,nullptr)!=GetWindowThreadProcessId(self->parent,nullptr))) {
        log(self.get(),"OpenGL: presentation child must belong to the GLFW event thread.");self->window=nullptr;
    }
    if(!self->window) {rsf_gl_close(self->sharing);return RSF_BACKEND_ERROR_INIT_FAILED;}
    rsf_fg12_setup fg{};fg.struct_size=sizeof(fg);fg.backend=0;fg.max_generated_frames=15;
    fg.runtime_directory_utf8=self->fsr3.c_str();fg.options={sizeof(fg.options),RSF_FG_OFF,1,RSF_REFLEX_OFF,0,0};
    fg.units_to_meters=1;fg.log=log;fg.user=self.get();fg.accept_window=accept;fg.runtime_switching=1;
    fg.ui_mode=RSF_UI_MODE_BACKBUFFER_HUDLESS;fg.fsr3_directory_utf8=self->fsr3.c_str();fg.fsr4_directory_utf8=self->fsr4.c_str();
    fg.xess_directory_utf8=self->xess.c_str();fg.streamline_directory_utf8=self->streamline.c_str();
    fg.engine_type=RSF_DLSS_ENGINE_CUSTOM;fg.engine_version_utf8=self->version.c_str();fg.project_id_utf8=self->project.c_str();
    result=rsf_fg12_install(&fg);if(result!=RSF_BACKEND_OK) {if(self->own_window)rsf_gl_window_destroy(self->window);rsf_gl_close(self->sharing);return result;}
    ComPtr<IDXGIFactory2> factory;ComPtr<IDXGISwapChain1> chain;
    DXGI_SWAP_CHAIN_DESC1 desc{};desc.Width=desc.Height=32;desc.Format=DXGI_FORMAT_R8G8B8A8_UNORM;
    desc.SampleDesc.Count=1;desc.BufferUsage=DXGI_USAGE_RENDER_TARGET_OUTPUT;desc.BufferCount=3;
    desc.SwapEffect=DXGI_SWAP_EFFECT_FLIP_DISCARD;desc.AlphaMode=DXGI_ALPHA_MODE_IGNORE;
    auto chain_result=CreateDXGIFactory1(IID_PPV_ARGS(&factory));
    if(SUCCEEDED(chain_result))chain_result=factory->CreateSwapChainForHwnd(self->queue,self->window,&desc,nullptr,nullptr,&chain);
    if(SUCCEEDED(chain_result))chain_result=chain.As(&self->chain);
    if(FAILED(chain_result)||!rsf_d3d11_present_is_owner(self->chain.Get())) {
        char message[160];std::snprintf(message,sizeof(message),"OpenGL: DXGI creation result=0x%08x, intercepted=%u",static_cast<uint32_t>(chain_result),rsf_d3d11_present_is_owner(self->chain.Get()));log(self.get(),message);
        // Hooks retain setup.user for process lifetime. Keep the owner on initialization failure.
        log(self.get(),"OpenGL: native presentation refused; GL presentation must be retained.");self.release();return RSF_BACKEND_ERROR_INIT_FAILED;
    }
    factory->MakeWindowAssociation(self->window,DXGI_MWA_NO_ALT_ENTER);
    *out=self.release();return RSF_BACKEND_OK;
} catch(...) {return RSF_BACKEND_ERROR_INIT_FAILED;}
extern "C" rsf_backend_result rsf_gl_pipeline_configure(rsf_gl_pipeline* self,const rsf_gl_pipeline_options* options,uint32_t width,uint32_t height) try {
    if(!self||!options||options->struct_size<sizeof(*options)||!width||!height||options->sr_backend>RSF_SR_AUTO||
        options->sr_backend==RSF_SR_SPATIAL||options->quality>5||options->generation.struct_size<sizeof(options->generation))return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    const bool rebuild=self->width!=width||self->height!=height||self->options.sr_backend!=options->sr_backend||self->options.quality!=options->quality||!self->sr;
    self->options=*options;self->status.requested_sr=options->sr_backend;self->status.requested_fg=options->fg_backend;
    rsf_fg12_options(&options->generation);
    uint32_t requested_fg=options->fg_backend;
    if(requested_fg==RSF_FG_BACKEND_AUTO) {
        ComPtr<IDXGIFactory4> factory;ComPtr<IDXGIAdapter1> adapter;DXGI_ADAPTER_DESC1 description{};
        if(SUCCEEDED(CreateDXGIFactory1(IID_PPV_ARGS(&factory)))&&SUCCEEDED(factory->EnumAdapterByLuid(self->device->GetAdapterLuid(),IID_PPV_ARGS(&adapter)))&&
            SUCCEEDED(adapter->GetDesc1(&description)))requested_fg=rsf_fg_default_backend(description.VendorId,(description.Flags&DXGI_ADAPTER_FLAG_SOFTWARE)!=0);
        else requested_fg=0;
    }
    if(requested_fg!=rsf_d3d11_present_backend())self->status.fg_switch_result=rsf_d3d11_present_request(requested_fg);
    if(!rebuild)return RSF_BACKEND_OK;
    auto result=rsf_gl_drain(self->sharing);if(result!=RSF_BACKEND_OK)return result;
    release_surfaces(*self);self->width=width;self->height=height;
    if(!surface_message(self->window,surface_resize,width,height))return RSF_BACKEND_ERROR_FEATURE_FAILED;
    if(FAILED(self->chain->ResizeBuffers(3,width,height,DXGI_FORMAT_R8G8B8A8_UNORM,0)))return RSF_BACKEND_ERROR_FEATURE_FAILED;
    uint32_t selected=options->sr_backend;
    if(selected==RSF_SR_AUTO) {
        // Query actual backends, retaining the first usable session and its SDK plan.
        for(uint32_t candidate:{RSF_SR_DLSS,RSF_SR_FSR4,RSF_SR_XESS,RSF_SR_FSR3,RSF_SR_FSR2}) {
            result=create_sr(*self,candidate,&self->sr);if(result==RSF_BACKEND_OK) {selected=candidate;break;}
        }
    } else result=create_sr(*self,selected,&self->sr);
    self->status.sr_result=result;
    if(!self->sr) {selected=RSF_SR_NONE;result=create_sr(*self,selected,&self->sr);}
    if(result!=RSF_BACKEND_OK)return result;
    self->status.effective_sr=selected;rsf_sr12_plan(self->sr,&self->render_width,&self->render_height);
    for(size_t i=0;i<self->surfaces.size();++i) {
        const uint32_t format=i==1?RSF_GL_R32F:i==2?RSF_GL_RG16F:i>=4?RSF_GL_RGBA8:RSF_GL_RGBA16F;
        result=rsf_gl_texture_create(self->sharing,i<3?self->render_width:width,i<3?self->render_height:height,format,&self->surfaces[i]);
        if(result!=RSF_BACKEND_OK) {release_surfaces(*self);return result;}
        auto& info=self->textures[i];info.struct_size=sizeof(info);rsf_gl_texture_get(self->surfaces[i],&info);
    }
    ++self->generation;return RSF_BACKEND_OK;
} catch(...) {return RSF_BACKEND_ERROR_FEATURE_FAILED;}
extern "C" rsf_backend_result rsf_gl_pipeline_get_surfaces(rsf_gl_pipeline* self,rsf_gl_pipeline_surfaces* out) {
    if(!self||!self->sr||!out||out->struct_size<sizeof(*out))return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    *out={sizeof(*out),self->render_width,self->render_height,self->width,self->height,self->textures[0].texture,self->textures[1].texture,
        self->textures[2].texture,self->textures[3].texture,self->textures[4].texture,self->textures[5].texture};return RSF_BACKEND_OK;
}
extern "C" void rsf_gl_pipeline_cpu(rsf_gl_pipeline* self,uint64_t frame,uint32_t stage) {
    if(!self)return;LARGE_INTEGER now{},frequency{};QueryPerformanceCounter(&now);QueryPerformanceFrequency(&frequency);
    rsf_game_cpu_event event{sizeof(event),stage,self->session,frame,static_cast<uint64_t>(now.QuadPart),static_cast<uint64_t>(frequency.QuadPart),0,0,0};rsf_fg12_cpu(&event);
}
extern "C" rsf_backend_result rsf_gl_pipeline_evaluate(rsf_gl_pipeline* self,const rsf_game_render_pass* input) {
    if(!self||!self->sr||!input||input->struct_size<sizeof(*input))return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    auto pass=*input;pass.session_id=self->session;pass.resource_generation=self->generation;pass.color_input=self->textures[0].resource;
    pass.depth=self->textures[1].resource;pass.motion=self->textures[2].resource;pass.color_output=self->textures[3].resource;
    void* commands=nullptr;uint32_t slot=0;auto result=rsf_gl_begin(self->sharing,4,self->surfaces.data(),&commands,&slot);if(result!=RSF_BACKEND_OK)return result;
    auto* list=static_cast<ID3D12GraphicsCommandList*>(commands);
    for(size_t i=0;i<4;++i)barrier(list,self->textures[i].resource,D3D12_RESOURCE_STATE_COMMON,i==3?D3D12_RESOURCE_STATE_COPY_DEST:D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
    result=rsf_sr12_evaluate(self->sr,commands,&pass,slot);
    for(size_t i=0;i<4;++i)barrier(list,self->textures[i].resource,i==3?D3D12_RESOURCE_STATE_COPY_DEST:D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,D3D12_RESOURCE_STATE_COMMON);
    const auto submitted=rsf_gl_submit(self->sharing,4,self->surfaces.data());
    if(submitted!=RSF_BACKEND_OK)return submitted;rsf_fg12_submitted(self->session,pass.source_frame_id);
    self->status.sr_result=result;if(result==RSF_BACKEND_OK)++self->status.evaluated;return result;
}
extern "C" rsf_backend_result rsf_gl_pipeline_hudless(rsf_gl_pipeline* self,const rsf_game_render_pass* input) {
    if(!self||!self->sr||!input||input->struct_size<sizeof(*input))return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    auto pass=*input;pass.session_id=self->session;pass.color_input=self->textures[4].resource;
    void* commands=nullptr;uint32_t slot=0;auto result=rsf_gl_begin(self->sharing,1,&self->surfaces[4],&commands,&slot);if(result!=RSF_BACKEND_OK)return result;
    auto* list=static_cast<ID3D12GraphicsCommandList*>(commands);barrier(list,pass.color_input,D3D12_RESOURCE_STATE_COMMON,D3D12_RESOURCE_STATE_RENDER_TARGET);
    rsf_fg12_hudless(commands,&pass);barrier(list,pass.color_input,D3D12_RESOURCE_STATE_RENDER_TARGET,D3D12_RESOURCE_STATE_COMMON);
    result=rsf_gl_submit(self->sharing,1,&self->surfaces[4]);if(result==RSF_BACKEND_OK)rsf_fg12_submitted(self->session,pass.source_frame_id);return result;
}
extern "C" rsf_backend_result rsf_gl_pipeline_present(rsf_gl_pipeline* self,uint64_t frame,uint32_t vsync) {
    if(!self||!self->sr)return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    ComPtr<ID3D12Resource> buffer;if(FAILED(self->chain->GetBuffer(self->chain->GetCurrentBackBufferIndex(),IID_PPV_ARGS(&buffer))))return RSF_BACKEND_ERROR_FEATURE_FAILED;
    void* commands=nullptr;uint32_t slot=0;auto result=rsf_gl_begin(self->sharing,1,&self->surfaces[5],&commands,&slot);if(result!=RSF_BACKEND_OK)return result;
    auto* list=static_cast<ID3D12GraphicsCommandList*>(commands);
    barrier(list,self->textures[5].resource,D3D12_RESOURCE_STATE_COMMON,D3D12_RESOURCE_STATE_COPY_SOURCE);barrier(list,buffer.Get(),D3D12_RESOURCE_STATE_PRESENT,D3D12_RESOURCE_STATE_COPY_DEST);
    list->CopyResource(buffer.Get(),static_cast<ID3D12Resource*>(self->textures[5].resource));
    barrier(list,buffer.Get(),D3D12_RESOURCE_STATE_COPY_DEST,D3D12_RESOURCE_STATE_PRESENT);barrier(list,self->textures[5].resource,D3D12_RESOURCE_STATE_COPY_SOURCE,D3D12_RESOURCE_STATE_COMMON);
    result=rsf_gl_submit(self->sharing,1,&self->surfaces[5]);if(result!=RSF_BACKEND_OK)return result;
    rsf_game_render_pass pass{};pass.struct_size=sizeof(pass);pass.role=RSF_GAME_RENDER_WINDOW;pass.session_id=self->session;
    pass.source_frame_id=frame;pass.view_key=1;pass.swapchain=self->chain.Get();rsf_fg12_window(&pass);
    const auto presented=self->chain->Present(vsync?1:0,0);
    if(SUCCEEDED(presented)&&!self->shown) {ShowWindowAsync(self->window,SW_SHOWNOACTIVATE);self->shown=true;}
    return SUCCEEDED(presented)?RSF_BACKEND_OK:RSF_BACKEND_ERROR_FEATURE_FAILED;
}
extern "C" rsf_backend_result rsf_gl_pipeline_status_get(rsf_gl_pipeline* self,rsf_gl_pipeline_status* out) {
    if(!self||!out||out->struct_size<sizeof(*out))return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    *out=self->status;out->struct_size=sizeof(*out);out->effective_fg=rsf_d3d11_present_backend();out->fg_switch_result=rsf_d3d11_present_switch_result();
    out->generation.struct_size=sizeof(out->generation);rsf_fg12_status(&out->generation);return RSF_BACKEND_OK;
}
extern "C" rsf_backend_result rsf_gl_pipeline_close(rsf_gl_pipeline* self) {
    if(!self)return RSF_BACKEND_OK;const auto drained=rsf_gl_drain(self->sharing);if(drained!=RSF_BACKEND_OK)return drained;
    // DLSS SR borrows the chain's Streamline host, so release SR before that host.
    release_surfaces(*self);self->chain.Reset();const auto closed=rsf_gl_close(self->sharing);if(closed!=RSF_BACKEND_OK)return closed;self->sharing=nullptr;
    if(self->own_window)rsf_gl_window_destroy(self->window);self->window=nullptr;
    // Installed presentation callbacks are process-lifetime. Keep this small owner alive.
    return RSF_BACKEND_OK;
}

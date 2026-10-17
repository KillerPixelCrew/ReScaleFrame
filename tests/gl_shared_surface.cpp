// SPDX-License-Identifier: GPL-3.0-only
#define RSF_STREAMLINE_HOST_BUILD
#include "../games/minecraft/native/input_observer.h"
#include <rescaleframe/gl_shared_surface.h>
#include <rescaleframe/gl_pipeline.h>
#include <rescaleframe/sr_session.h>
#include <windows.h>
#include <GL/gl.h>
#include <d3d12.h>
#include <d3d12sdklayers.h>
#include <wrl/client.h>
#include <cstdio>
#include <cmath>
#include <stdexcept>
#include <cstdlib>
#include <atomic>
#include <thread>
#include <exception>
using Microsoft::WRL::ComPtr;
extern "C" __declspec(dllexport) DWORD NvOptimusEnablement=1;
extern "C" __declspec(dllexport) int AmdPowerXpressRequestHighPerformance=1;
namespace {
void require(bool value,const char* message) { if(!value)throw std::runtime_error(message); }
void log(void*,const char* text) { std::puts(text); }
void APIENTRY debug_gl(GLenum,GLenum,GLuint,GLenum,GLsizei,const char* text,const void*) { std::fprintf(stderr,"GL: %s\n",text); }
void transition(ID3D12GraphicsCommandList* list,ID3D12Resource* resource,D3D12_RESOURCE_STATES from,D3D12_RESOURCE_STATES to) {
    D3D12_RESOURCE_BARRIER barrier{};barrier.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    barrier.Transition={resource,D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,from,to};list->ResourceBarrier(1,&barrier);
}
struct Window {
    HWND window=nullptr;HDC dc=nullptr;HGLRC context=nullptr;
    Window() {
        WNDCLASSW type{};type.lpfnWndProc=DefWindowProcW;type.hInstance=GetModuleHandleW(nullptr);type.lpszClassName=L"RSFHiddenGLTest";
        require(RegisterClassW(&type)!=0,"register hidden GL window");
        window=CreateWindowW(type.lpszClassName,L"RSF hidden GPU test",WS_POPUP,0,0,32,32,nullptr,nullptr,type.hInstance,nullptr);
        require(window!=nullptr,"create hidden GL window");dc=GetDC(window);
        PIXELFORMATDESCRIPTOR format{};format.nSize=sizeof(format);format.nVersion=1;
        format.dwFlags=PFD_DRAW_TO_WINDOW|PFD_SUPPORT_OPENGL|PFD_DOUBLEBUFFER;format.iPixelType=PFD_TYPE_RGBA;
        format.cColorBits=32;format.cDepthBits=24;
        const int pixel=ChoosePixelFormat(dc,&format);require(pixel!=0&&SetPixelFormat(dc,pixel,&format),"GL pixel format");
        context=wglCreateContext(dc);require(context&&wglMakeCurrent(dc,context),"current hidden GL context");
        std::printf("OpenGL interop GPU: %s / %s\n",glGetString(GL_RENDERER),glGetString(GL_VERSION));
    }
    ~Window() { wglMakeCurrent(nullptr,nullptr);if(context)wglDeleteContext(context);if(dc)ReleaseDC(window,dc);if(window)DestroyWindow(window); }
};
}
int main() try {
    wchar_t enabled[4]{};if(!GetEnvironmentVariableW(L"RSF_RUN_GL_INTEROP_TESTS",enabled,4)||enabled[0]!=L'1')return 77;
    Window window;ComPtr<ID3D12Debug> debug;
    auto debug_callback=reinterpret_cast<void(APIENTRY*)(decltype(&debug_gl),const void*)>(wglGetProcAddress("glDebugMessageCallback"));
    if(debug_callback&&!GetEnvironmentVariableW(L"RSF_GL_NO_DEBUG",enabled,4)) {glEnable(0x92e0);glEnable(0x8242);debug_callback(debug_gl,nullptr);}
    if(SUCCEEDED(D3D12GetDebugInterface(IID_PPV_ARGS(&debug))))debug->EnableDebugLayer();
    rsf_gl_setup setup{sizeof(setup),1,nullptr,nullptr,log,nullptr};rsf_gl_context* context=nullptr;
    require(rsf_gl_open(&setup,&context)==RSF_BACKEND_OK,"open same-adapter GL/D3D12 sharing");
    void* device_pointer=nullptr;void* queue_pointer=nullptr;
    require(rsf_gl_graphics(context,&device_pointer,&queue_pointer)==RSF_BACKEND_OK,"borrow D3D12 device");
    auto* device=static_cast<ID3D12Device*>(device_pointer);
    for(uint32_t size: {32u,854u,1920u,2560u,32u}) {
        for(uint32_t format: {RSF_GL_RGBA16F,RSF_GL_R32F,RSF_GL_RG16F,RSF_GL_RGBA8}) {
            rsf_gl_texture* probe=nullptr;
            require(rsf_gl_texture_create(context,size,size*9/16,format,&probe)==RSF_BACKEND_OK,"repeated import of scene/depth/motion/output formats");
            require(rsf_gl_drain(context)==RSF_BACKEND_OK,"retire format probe");rsf_gl_texture_destroy(probe);
        }
    }
    rsf_gl_texture* shared=nullptr;
    require(rsf_gl_texture_create(context,32,32,RSF_GL_RGBA16F,&shared)==RSF_BACKEND_OK,"create shared GL/D3D12 texture");
    rsf_gl_texture_info info{};info.struct_size=sizeof(info);require(rsf_gl_texture_get(shared,&info)==RSF_BACKEND_OK,"shared texture identity");
    auto gen=reinterpret_cast<void(APIENTRY*)(GLsizei,GLuint*)>(wglGetProcAddress("glGenFramebuffers"));
    auto bind=reinterpret_cast<void(APIENTRY*)(GLenum,GLuint)>(wglGetProcAddress("glBindFramebuffer"));
    auto attach=reinterpret_cast<void(APIENTRY*)(GLenum,GLenum,GLenum,GLuint,GLint)>(wglGetProcAddress("glFramebufferTexture2D"));
    auto check=reinterpret_cast<GLenum(APIENTRY*)(GLenum)>(wglGetProcAddress("glCheckFramebufferStatus"));
    auto remove=reinterpret_cast<void(APIENTRY*)(GLsizei,const GLuint*)>(wglGetProcAddress("glDeleteFramebuffers"));
    require(gen&&bind&&attach&&check&&remove,"framebuffer API");GLuint framebuffer=0;gen(1,&framebuffer);bind(0x8d40,framebuffer);
    attach(0x8d40,0x8ce0,GL_TEXTURE_2D,info.texture,0);require(check(0x8d40)==0x8cd5,"shared GL framebuffer");
    glViewport(0,0,32,32);glClearColor(.25f,.5f,.75f,1);glClear(GL_COLOR_BUFFER_BIT);
    require(glGetError()==GL_NO_ERROR,"GL clear shared resource");
    auto* texture=static_cast<ID3D12Resource*>(info.resource);
    const auto description=texture->GetDesc();D3D12_PLACED_SUBRESOURCE_FOOTPRINT footprint{};UINT rows=0;UINT64 row_size=0,total=0;
    device->GetCopyableFootprints(&description,0,1,0,&footprint,&rows,&row_size,&total);
    D3D12_HEAP_PROPERTIES heap{};heap.Type=D3D12_HEAP_TYPE_READBACK;
    D3D12_RESOURCE_DESC buffer{};buffer.Dimension=D3D12_RESOURCE_DIMENSION_BUFFER;buffer.Width=total;
    buffer.Height=buffer.DepthOrArraySize=buffer.MipLevels=1;buffer.SampleDesc.Count=1;buffer.Layout=D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    ComPtr<ID3D12Resource> readback;
    require(SUCCEEDED(device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&buffer,D3D12_RESOURCE_STATE_COPY_DEST,nullptr,IID_PPV_ARGS(&readback))),"readback for test assertions");
    void* pointer=nullptr;uint32_t slot=0;
    require(rsf_gl_begin(context,1,&shared,&pointer,&slot)==RSF_BACKEND_OK,"GL release and D3D12 GPU wait");
    auto* list=static_cast<ID3D12GraphicsCommandList*>(pointer);
    transition(list,texture,D3D12_RESOURCE_STATE_COMMON,D3D12_RESOURCE_STATE_COPY_SOURCE);
    D3D12_TEXTURE_COPY_LOCATION from{};from.pResource=texture;from.Type=D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
    D3D12_TEXTURE_COPY_LOCATION to{};to.pResource=readback.Get();to.Type=D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT;to.PlacedFootprint=footprint;
    list->CopyTextureRegion(&to,0,0,0,&from,nullptr);transition(list,texture,D3D12_RESOURCE_STATE_COPY_SOURCE,D3D12_RESOURCE_STATE_COMMON);
    require(rsf_gl_submit(context,1,&shared)==RSF_BACKEND_OK&&rsf_gl_drain(context)==RSF_BACKEND_OK,"D3D12 completion signal and GL acquire");
    void* mapped=nullptr;D3D12_RANGE range{0,static_cast<SIZE_T>(total)};
    require(SUCCEEDED(readback->Map(0,&range,&mapped)),"map verification readback");
    const auto* pixel=static_cast<const uint16_t*>(mapped);
    require(pixel[0]==0x3400&&pixel[1]==0x3800&&pixel[2]==0x3a00&&pixel[3]==0x3c00,"GL clear must reach D3D12 unchanged");
    D3D12_RANGE written{0,0};readback->Unmap(0,&written);
    D3D12_DESCRIPTOR_HEAP_DESC descriptors{};descriptors.Type=D3D12_DESCRIPTOR_HEAP_TYPE_RTV;descriptors.NumDescriptors=1;
    ComPtr<ID3D12DescriptorHeap> rtv;
    require(SUCCEEDED(device->CreateDescriptorHeap(&descriptors,IID_PPV_ARGS(&rtv))),"RTV heap");
    device->CreateRenderTargetView(texture,nullptr,rtv->GetCPUDescriptorHandleForHeapStart());
    require(rsf_gl_begin(context,1,&shared,&pointer,&slot)==RSF_BACKEND_OK,"second shared batch");list=static_cast<ID3D12GraphicsCommandList*>(pointer);
    transition(list,texture,D3D12_RESOURCE_STATE_COMMON,D3D12_RESOURCE_STATE_RENDER_TARGET);
    const float color[]{.8f,.2f,.4f,1};list->ClearRenderTargetView(rtv->GetCPUDescriptorHandleForHeapStart(),color,0,nullptr);
    transition(list,texture,D3D12_RESOURCE_STATE_RENDER_TARGET,D3D12_RESOURCE_STATE_COMMON);
    require(rsf_gl_submit(context,1,&shared)==RSF_BACKEND_OK,"D3D12 clear release");
    float result[4]{};glReadPixels(16,16,1,1,GL_RGBA,GL_FLOAT,result);
    for(size_t i=0;i<4;++i)require(std::abs(result[i]-color[i])<.001f,"D3D12 clear must reach GL unchanged");
    require(glGetError()==GL_NO_ERROR&&rsf_gl_drain(context)==RSF_BACKEND_OK,"retire shared GPU work");
    bind(0x8d40,0);remove(1,&framebuffer);rsf_gl_texture_destroy(shared);require(rsf_gl_close(context)==RSF_BACKEND_OK,"close shared graphics");
    std::puts("PASS: OpenGL -> D3D12 and D3D12 -> OpenGL pixels, shared memory, GPU fence ordering.");
    if(const char* runtime=std::getenv("RSF_GL_TEST_RUNTIME")) {
        // Minecraft has presented its loading screen before the first runTick hook.
        require(SwapBuffers(window.dc)!=0,"present WGL before adopting native presentation");
        void* child=rsf_gl_window_create(window.window);require(child!=nullptr,"create native child on GLFW owner thread");
        auto& observer=MinecraftInputObserver::instance();
        require(observer.install(window.window,static_cast<HWND>(child)),"observe GLFW owner's dequeued messages");
        const UINT probe=RegisterWindowMessageW(L"ReScaleFrame.HiddenInputProbe");
        observer.set_message(probe);
        require(PostMessageW(window.window,probe,0,0)!=0,"post ping to hidden GLFW parent");
        MSG probe_message{};
        require(PeekMessageW(&probe_message,window.window,probe,probe,PM_NOREMOVE)!=0,"peek tool message without consuming");
        require(observer.consume()==0,"a non-removing peek must not sample the ping");
        require(PeekMessageW(&probe_message,window.window,probe,probe,PM_REMOVE)!=0,"dequeue tool message");
        require(observer.consume()==probe&&observer.consume()==0,"dequeued ping reaches exactly one next simulation");
        require(PostThreadMessageW(GetCurrentThreadId(),probe,0,0)!=0,"post SDK-style thread ping to hidden event owner");
        require(PeekMessageW(&probe_message,nullptr,probe,probe,PM_REMOVE)!=0&&observer.consume()==probe,"thread-targeted tool pings share the same input boundary");
        require(SendMessageW(static_cast<HWND>(child),WM_NCHITTEST,0,0)==HTTRANSPARENT,"native child must pass hit tests to the GLFW parent");
        require(wglMakeCurrent(nullptr,nullptr)!=0,"release GL context before threaded rendering");
        std::atomic<bool> done=false;std::exception_ptr render_error;
        std::thread graphics([&] {
        try {
        require(wglMakeCurrent(window.dc,window.context)!=0,"adopt GL context on separate render thread");
        require(rsf_gl_window_create(window.window)==nullptr,"reject window creation on the wrong event thread");
        rsf_gl_pipeline_setup pipeline_setup{sizeof(pipeline_setup),window.window,runtime,log,nullptr,"Minecraft 1.21.1","11e3ed7f-06c6-4863-a54d-2b790f2176f2",child};
        rsf_gl_pipeline* pipeline=nullptr;require(rsf_gl_pipeline_open(&pipeline_setup,&pipeline)==RSF_BACKEND_OK,"GL window must accept framework DXGI presentation");
        uint64_t frame=0;
        GLuint held_framebuffer=0;gen(1,&held_framebuffer);
        const uint32_t scenarios[][4]={{0,0,1280,720},{2,0,1280,720},{3,0,1280,720},{5,0,1280,720},{1,0,1280,720},
            {0,3,1280,720},{3,5,1280,720},{1,1,1280,720},{1,1,854,480},{1,1,1920,1080},{1,1,2560,1494},{1,1,1280,720},{0,0,1280,720}};
        for(const auto& scenario:scenarios) {
            const uint32_t backend=scenario[0],generation_backend=scenario[1],width=scenario[2],height=scenario[3];
            rsf_gl_pipeline_options options{};options.struct_size=sizeof(options);options.sr_backend=backend;options.quality=1;
            options.fg_backend=generation_backend;
            options.generation={sizeof(options.generation),generation_backend?RSF_FG_FIXED:RSF_FG_OFF,1,RSF_REFLEX_ON,0,0};
            // Another renderer's sticky error must not be called an import failure.
            glEnable(0xdeadbeef);
            require(rsf_gl_pipeline_configure(pipeline,&options,width,height)==RSF_BACKEND_OK,"configure native GL pipeline");
            rsf_gl_pipeline_surfaces surfaces{};surfaces.struct_size=sizeof(surfaces);
            require(rsf_gl_pipeline_get_surfaces(pipeline,&surfaces)==RSF_BACKEND_OK,"get native GL surfaces");
            rsf_gl_pipeline_status status{};status.struct_size=sizeof(status);rsf_gl_pipeline_status_get(pipeline,&status);
            require(status.effective_sr==backend,"requested SR backend must actually open in GPU harness");
            GLuint fbo=0;gen(1,&fbo);bind(0x8d40,fbo);
            for(uint32_t surface: {surfaces.scene,surfaces.depth,surfaces.motion,surfaces.hudless,surfaces.composed}) {
                attach(0x8d40,0x8ce0,GL_TEXTURE_2D,surface,0);require(check(0x8d40)==0x8cd5,"native pipeline surface FBO");
                glViewport(0,0,static_cast<GLsizei>(width),static_cast<GLsizei>(height));glClearColor(.25f,.5f,.75f,1);glClear(GL_COLOR_BUFFER_BIT);
            }
            require(rsf_gl_pipeline_present(pipeline,0,0)==RSF_BACKEND_ERROR_INVALID_ARGUMENT,"menus must not replace their input frame ID with zero");
            for(uint32_t menu=0;menu<32;++menu) {
                ++frame;
                for(uint32_t stage:{0u,6u})rsf_gl_pipeline_cpu(pipeline,frame,stage);
                require(rsf_gl_pipeline_input(pipeline,frame,7,0)==RSF_BACKEND_OK,"sample keyboard mouse and controller on the acquired CPU token");
                require(rsf_gl_pipeline_input(pipeline,frame,4,0)==RSF_BACKEND_OK,"multiple polls retain one controller sample marker");
                if(const auto ping=rsf_gl_pipeline_pcl_message(pipeline))
                    require(rsf_gl_pipeline_input(pipeline,frame,0,ping)==RSF_BACKEND_OK,"SDK registered latency ping uses the consuming input frame");
                for(uint32_t stage:{1u,3u})rsf_gl_pipeline_cpu(pipeline,frame,stage);
                if(auto* host=rsf_d3d11_present_host())require(rsf_streamline_host_token(host,frame)!=nullptr,"menu frame must acquire a real SDK token");
                require(rsf_gl_pipeline_present(pipeline,frame,0)==RSF_BACKEND_OK,"menu present closes its real CPU frame without a scene");
                if(auto* host=rsf_d3d11_present_host())require(rsf_streamline_host_token(host,frame)==nullptr,"menu present must release its SDK token");
                require(rsf_gl_pipeline_input(pipeline,frame,1,0)!=RSF_BACKEND_OK,"post-present input must never reuse the closed SDK token");
            }
            ++frame;
            for(uint32_t stage:{0u,6u,1u,3u})rsf_gl_pipeline_cpu(pipeline,frame,stage);
            require(rsf_gl_pipeline_abort(pipeline,frame)==RSF_BACKEND_OK,"incomplete resized frame closes its CPU ownership");
            if(auto* host=rsf_d3d11_present_host())require(rsf_streamline_host_token(host,frame)==nullptr,"aborted resize frame must release its SDK token");
            rsf_game_render_pass pass{};pass.struct_size=sizeof(pass);pass.history_key=pass.view_key=1;pass.camera_valid=1;pass.screen=RSF_SCREEN_FLIGHT;
            auto& camera=pass.camera;camera.struct_size=sizeof(camera);camera.abi_version=RSF_GAME_FRAME_ABI_VERSION;
            for(size_t i=0;i<16;i+=5)camera.view_to_clip[i]=camera.clip_to_view[i]=camera.view_to_world[i]=camera.world_to_view[i]=camera.clip_to_previous_clip[i]=1;
            camera.render_width=surfaces.render_width;camera.render_height=surfaces.render_height;camera.output_width=width;camera.output_height=height;
            camera.near_plane=.05f;camera.far_plane=1024;camera.vertical_fov_radians=1;camera.frame_time_seconds=.016f;
            pass.motion_to_uv[0]=1.0f/surfaces.render_width;pass.motion_to_uv[1]=1.0f/surfaces.render_height;
            for(uint32_t iteration=0;iteration<(generation_backend?16u:3u);++iteration) {
                pass.native_frame=pass.source_frame_id=++frame;pass.flags=iteration?0:RSF_GAME_RENDER_RESET;
                for(uint32_t stage:{0u,6u,1u,3u})rsf_gl_pipeline_cpu(pipeline,frame,stage);
                require(rsf_gl_pipeline_evaluate(pipeline,&pass)==RSF_BACKEND_OK,"real vendor SR evaluate from GL shared inputs");
                require(rsf_gl_pipeline_hudless(pipeline,&pass)==RSF_BACKEND_OK,"capture actual HUDless surface");
                require(rsf_gl_pipeline_present(pipeline,frame,0)==RSF_BACKEND_OK,"present composed OpenGL image through native DXGI owner");
            }
            if(backend) {
                bind(0x8d40,fbo);attach(0x8d40,0x8ce0,GL_TEXTURE_2D,surfaces.output,0);
                const auto right=static_cast<GLint>(width-1),top=static_cast<GLint>(height-1);
                const GLint locations[][2]={{0,0},{right,0},{0,top},{right,top},{right/2,top/2}};
                for(const auto& location:locations) {
                    float color_pixel[4]{};glReadPixels(location[0],location[1],1,1,GL_RGBA,GL_FLOAT,color_pixel);
                    require(std::isfinite(color_pixel[0])&&std::abs(color_pixel[0]-.25f)<.03f&&
                        std::isfinite(color_pixel[1])&&std::abs(color_pixel[1]-.5f)<.03f&&
                        std::isfinite(color_pixel[2])&&std::abs(color_pixel[2]-.75f)<.03f,"vendor output must preserve SDR colour across all output corners and centre");
                }
            }
            rsf_gl_pipeline_status_get(pipeline,&status);
            if(generation_backend) {
                std::printf("FG backend=%u effective=%u switch=%d result=%d reason=%u tags=%llu generated=%u\n",generation_backend,status.effective_fg,
                    status.fg_switch_result,status.generation.last_result,status.generation.reason,
                    static_cast<unsigned long long>(status.generation.tagged_frames),status.generation.vendor.effective_generated_frames);
                require(status.effective_fg==generation_backend&&status.generation.last_result==RSF_BACKEND_OK&&status.generation.reason==0&&status.generation.tagged_frames>0,
                    "real generation provider must prepare valid input/simulation/world/HUDless frames");
            }
            // Force a real unsupported allocation extent and retain a reference
            // to the last published surface, as Minecraft's FBO does at resize.
            bind(0x8d40,held_framebuffer);attach(0x8d40,0x8ce0,GL_TEXTURE_2D,surfaces.composed,0);
            if(backend==RSF_SR_DLSS) {
                require(rsf_gl_pipeline_configure(pipeline,&options,32768,64)!=RSF_BACKEND_OK,"reject unsupported allocation dimensions");
                rsf_gl_pipeline_surfaces retained{};retained.struct_size=sizeof(retained);rsf_gl_pipeline_get_surfaces(pipeline,&retained);
                require(retained.output==surfaces.output&&retained.composed==surfaces.composed&&glIsTexture(retained.composed),"refused resize must preserve every published resource");
            }
            bind(0x8d40,0);remove(1,&fbo);
            std::printf("PASS: native SR backend %u, plan %ux%u -> %ux%u, runtime switch and DXGI presentation\n",backend,surfaces.render_width,surfaces.render_height,width,height);
        }
        bind(0x8d40,0);remove(1,&held_framebuffer);
        require(rsf_gl_pipeline_close(pipeline)==RSF_BACKEND_OK,"close GL native pipeline");
        } catch(...) {render_error=std::current_exception();}
        wglMakeCurrent(nullptr,nullptr);done.store(true,std::memory_order_release);
        });
        while(!done.load(std::memory_order_acquire)) {
            MSG message{};
            while(PeekMessageW(&message,nullptr,0,0,PM_REMOVE)) {TranslateMessage(&message);DispatchMessageW(&message);}
            MsgWaitForMultipleObjects(0,nullptr,FALSE,1,QS_ALLINPUT);
        }
        graphics.join();observer.close();require(rsf_gl_window_destroy(child)!=0,"destroy native child on its owner thread");
        if(render_error)std::rethrow_exception(render_error);
        std::puts("PASS: separate event/render threads, window ownership, transparent input hit testing, resize and shutdown.");
    }
    return 0;
} catch(const std::exception& error) {std::fprintf(stderr,"FAIL: %s\n",error.what());return 1;}

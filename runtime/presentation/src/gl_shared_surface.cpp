// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/gl_shared_surface.h>
#include <windows.h>
#include <GL/gl.h>
#include <d3d12.h>
#include <dxgi1_4.h>
#include <wrl/client.h>
#include <array>
#include <cstring>
#include <memory>
#include <cstdio>
using Microsoft::WRL::ComPtr;
namespace {
constexpr GLenum device_luid=0x9599, memory_dedicated=0x9581, d3d12_resource=0x958a;
constexpr GLenum d3d12_fence=0x9594, fence_value=0x9595, layout_general=0x958d;
struct Gl {
    const GLubyte* (APIENTRY* string_index)(GLenum,GLuint)=nullptr;
    void (APIENTRY* luid)(GLenum,GLubyte*)=nullptr;
    void (APIENTRY* create_memory)(GLsizei,GLuint*)=nullptr;
    void (APIENTRY* delete_memory)(GLsizei,const GLuint*)=nullptr;
    void (APIENTRY* memory_parameter)(GLuint,GLenum,const GLint*)=nullptr;
    void (APIENTRY* import_memory)(GLuint,uint64_t,GLenum,void*)=nullptr;
    void (APIENTRY* texture_storage)(GLenum,GLsizei,GLenum,GLsizei,GLsizei,GLuint,uint64_t)=nullptr;
    void (APIENTRY* create_semaphore)(GLsizei,GLuint*)=nullptr;
    void (APIENTRY* delete_semaphore)(GLsizei,const GLuint*)=nullptr;
    void (APIENTRY* import_semaphore)(GLuint,GLenum,void*)=nullptr;
    void (APIENTRY* semaphore_parameter)(GLuint,GLenum,const uint64_t*)=nullptr;
    void (APIENTRY* signal)(GLuint,GLuint,const GLuint*,GLuint,const GLuint*,const GLenum*)=nullptr;
    void (APIENTRY* wait)(GLuint,GLuint,const GLuint*,GLuint,const GLuint*,const GLenum*)=nullptr;
};
template<class T> bool resolve(T& target,const char* name) {
    target=reinterpret_cast<T>(wglGetProcAddress(name));
    return target&&reinterpret_cast<uintptr_t>(target)>3&&reinterpret_cast<intptr_t>(target)!=-1;
}
bool load(Gl& gl) {
    return resolve(gl.string_index,"glGetStringi")&&resolve(gl.luid,"glGetUnsignedBytevEXT")&&
        resolve(gl.create_memory,"glCreateMemoryObjectsEXT")&&resolve(gl.delete_memory,"glDeleteMemoryObjectsEXT")&&
        resolve(gl.memory_parameter,"glMemoryObjectParameterivEXT")&&resolve(gl.import_memory,"glImportMemoryWin32HandleEXT")&&
        resolve(gl.texture_storage,"glTexStorageMem2DEXT")&&resolve(gl.create_semaphore,"glGenSemaphoresEXT")&&
        resolve(gl.delete_semaphore,"glDeleteSemaphoresEXT")&&resolve(gl.import_semaphore,"glImportSemaphoreWin32HandleEXT")&&
        resolve(gl.semaphore_parameter,"glSemaphoreParameterui64vEXT")&&resolve(gl.signal,"glSignalSemaphoreEXT")&&resolve(gl.wait,"glWaitSemaphoreEXT");
}
bool extension(const Gl& gl,const char* name) {
    GLint count=0;glGetIntegerv(0x821d,&count);
    for(GLint i=0;i<count;++i) {
        const auto* value=gl.string_index(GL_EXTENSIONS,static_cast<GLuint>(i));
        if(value&&std::strcmp(reinterpret_cast<const char*>(value),name)==0)return true;
    }
    return false;
}
struct Slot { ComPtr<ID3D12CommandAllocator> allocator;ComPtr<ID3D12GraphicsCommandList> list;uint64_t completion=0; };
}
struct rsf_gl_context {
    Gl gl;HGLRC context=nullptr;DWORD thread=0;
    ComPtr<ID3D12Device> device;ComPtr<ID3D12CommandQueue> queue;ComPtr<ID3D12Fence> fence;
    HANDLE event=nullptr,fence_handle=nullptr;GLuint semaphore=0;uint64_t serial=0;
    std::array<Slot,3> slots;uint32_t next_slot=0,current_slot=0;bool recording=false;
    rsf_backend_log_fn log=nullptr;void* user=nullptr;
};
struct rsf_gl_texture {
    rsf_gl_context* owner=nullptr;GLuint texture=0,memory=0;uint32_t width=0,height=0,format=0;
    ComPtr<ID3D12Resource> resource;
    HANDLE handle=nullptr;
    ~rsf_gl_texture() {if(handle)CloseHandle(handle);}
};
namespace {
bool owner(const rsf_gl_context* self) { return self&&self->thread==GetCurrentThreadId()&&self->context==wglGetCurrentContext(); }
void message(rsf_gl_context& self,const char* text) { if(self.log)self.log(self.user,text); }
void failure(rsf_gl_context& self,const char* operation,uint32_t code) {
    char text[192];std::snprintf(text,sizeof(text),"OpenGL sharing: %s failed (0x%08x)",operation,code);message(self,text);
}
void prior_errors(rsf_gl_context& self) {
    // The game owns the context too. A sticky error from its previous frame must
    // be reported separately rather than attributed to the next interop import.
    for(GLenum error=glGetError();error!=GL_NO_ERROR;error=glGetError())failure(self,"pre-existing GL operation before interop",error);
}
bool wait_cpu(rsf_gl_context& self,uint64_t value) {
    if(self.fence->GetCompletedValue()>=value)return true;
    return SUCCEEDED(self.fence->SetEventOnCompletion(value,self.event))&&WaitForSingleObject(self.event,10000)==WAIT_OBJECT_0;
}
bool barriers(rsf_gl_context& self,uint32_t count,rsf_gl_texture* const* textures,std::array<GLuint,16>& names,std::array<GLenum,16>& layouts) {
    if(count>names.size()||(count&&!textures))return false;
    for(uint32_t i=0;i<count;++i) {
        if(!textures[i]||textures[i]->owner!=&self)return false;
        names[i]=textures[i]->texture;layouts[i]=layout_general;
    }
    return true;
}
}
extern "C" rsf_backend_result rsf_gl_open(const rsf_gl_setup* setup,rsf_gl_context** out) try {
    if(!out)return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    *out=nullptr;
    if(!setup||setup->struct_size<sizeof(*setup)||!wglGetCurrentContext()||bool(setup->device)!=bool(setup->queue))return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if(setup->abi_version!=1)return RSF_BACKEND_ERROR_ABI_MISMATCH;
    auto self=std::make_unique<rsf_gl_context>();
    self->context=wglGetCurrentContext();self->thread=GetCurrentThreadId();self->log=setup->log;self->user=setup->user;
    if(!load(self->gl)||!extension(self->gl,"GL_EXT_memory_object_win32")||!extension(self->gl,"GL_EXT_semaphore_win32")) {
        message(*self,"OpenGL sharing requires EXT_memory_object_win32 and EXT_semaphore_win32.");return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    }
    LUID luid{};self->gl.luid(device_luid,reinterpret_cast<GLubyte*>(&luid));
    if(glGetError()!=GL_NO_ERROR)return RSF_BACKEND_ERROR_FEATURE_FAILED;
    if(setup->device) {
        self->device=static_cast<ID3D12Device*>(setup->device);self->queue=static_cast<ID3D12CommandQueue*>(setup->queue);
        const auto native_luid=self->device->GetAdapterLuid();ComPtr<ID3D12Device> queue_device;
        if(std::memcmp(&luid,&native_luid,sizeof(luid))||FAILED(self->queue->GetDevice(IID_PPV_ARGS(&queue_device)))||queue_device.Get()!=self->device.Get())return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    } else {
        ComPtr<IDXGIFactory4> factory;ComPtr<IDXGIAdapter1> adapter;
        if(FAILED(CreateDXGIFactory1(IID_PPV_ARGS(&factory)))||FAILED(factory->EnumAdapterByLuid(luid,IID_PPV_ARGS(&adapter)))||
            FAILED(D3D12CreateDevice(adapter.Get(),D3D_FEATURE_LEVEL_11_0,IID_PPV_ARGS(&self->device))))return RSF_BACKEND_ERROR_NOT_SUPPORTED;
        D3D12_COMMAND_QUEUE_DESC desc{};desc.Type=D3D12_COMMAND_LIST_TYPE_DIRECT;
        if(FAILED(self->device->CreateCommandQueue(&desc,IID_PPV_ARGS(&self->queue))))return RSF_BACKEND_ERROR_FEATURE_FAILED;
    }
    if(FAILED(self->device->CreateFence(0,D3D12_FENCE_FLAG_SHARED,IID_PPV_ARGS(&self->fence))))return RSF_BACKEND_ERROR_FEATURE_FAILED;
    HANDLE shared=nullptr;
    if(FAILED(self->device->CreateSharedHandle(self->fence.Get(),nullptr,GENERIC_ALL,nullptr,&shared)))return RSF_BACKEND_ERROR_FEATURE_FAILED;
    self->fence_handle=shared;
    self->gl.create_semaphore(1,&self->semaphore);self->gl.import_semaphore(self->semaphore,d3d12_fence,shared);
    if(glGetError()!=GL_NO_ERROR) {self->gl.delete_semaphore(1,&self->semaphore);CloseHandle(shared);return RSF_BACKEND_ERROR_FEATURE_FAILED;}
    self->event=CreateEventW(nullptr,FALSE,FALSE,nullptr);
    if(!self->event) {self->gl.delete_semaphore(1,&self->semaphore);CloseHandle(shared);return RSF_BACKEND_ERROR_FEATURE_FAILED;}
    for(auto& slot:self->slots) {
        if(FAILED(self->device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT,IID_PPV_ARGS(&slot.allocator)))||
            FAILED(self->device->CreateCommandList(0,D3D12_COMMAND_LIST_TYPE_DIRECT,slot.allocator.Get(),nullptr,IID_PPV_ARGS(&slot.list)))||FAILED(slot.list->Close())) {
            self->gl.delete_semaphore(1,&self->semaphore);CloseHandle(shared);CloseHandle(self->event);return RSF_BACKEND_ERROR_FEATURE_FAILED;
        }
    }
    message(*self,"OpenGL sharing: exact context adapter, D3D12 resource memory, shared GPU timeline fence.");
    *out=self.release();return RSF_BACKEND_OK;
} catch(...) {return RSF_BACKEND_ERROR_FEATURE_FAILED;}
extern "C" rsf_backend_result rsf_gl_graphics(rsf_gl_context* self,void** device,void** queue) {
    if(!owner(self)||!device||!queue)return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    *device=self->device.Get();*queue=self->queue.Get();return RSF_BACKEND_OK;
}
extern "C" rsf_backend_result rsf_gl_texture_create(rsf_gl_context* self,uint32_t width,uint32_t height,uint32_t format,rsf_gl_texture** out) try {
    if(!out)return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    *out=nullptr;
    if(!owner(self)||!width||!height)return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    prior_errors(*self);
    DXGI_FORMAT native;GLenum internal;
    switch(format) {
    case RSF_GL_RGBA16F:native=DXGI_FORMAT_R16G16B16A16_FLOAT;internal=0x881a;break;
    case RSF_GL_R32F:native=DXGI_FORMAT_R32_FLOAT;internal=0x822e;break;
    case RSF_GL_RG16F:native=DXGI_FORMAT_R16G16_FLOAT;internal=0x822f;break;
    case RSF_GL_RGBA8:native=DXGI_FORMAT_R8G8B8A8_UNORM;internal=0x8058;break;
    default:return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    }
    auto texture=std::make_unique<rsf_gl_texture>();texture->owner=self;texture->width=width;texture->height=height;texture->format=format;
    D3D12_RESOURCE_DESC desc{};desc.Dimension=D3D12_RESOURCE_DIMENSION_TEXTURE2D;desc.Width=width;desc.Height=height;
    desc.DepthOrArraySize=desc.MipLevels=1;desc.Format=native;desc.SampleDesc.Count=1;
    desc.Flags=D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET|D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS|D3D12_RESOURCE_FLAG_ALLOW_SIMULTANEOUS_ACCESS;
    D3D12_HEAP_PROPERTIES heap{};heap.Type=D3D12_HEAP_TYPE_DEFAULT;heap.CreationNodeMask=heap.VisibleNodeMask=1;
    const auto created=self->device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_SHARED,&desc,D3D12_RESOURCE_STATE_COMMON,nullptr,IID_PPV_ARGS(&texture->resource));
    if(FAILED(created)) {failure(*self,"shared D3D12 resource",static_cast<uint32_t>(created));return RSF_BACKEND_ERROR_FEATURE_FAILED;}
    HANDLE shared=nullptr;
    if(FAILED(self->device->CreateSharedHandle(texture->resource.Get(),nullptr,GENERIC_ALL,nullptr,&shared)))return RSF_BACKEND_ERROR_FEATURE_FAILED;
    // Keep the NT handle through the imported object's lifetime. Some drivers defer
    // the import to their GL server thread, beyond this entry point's return.
    texture->handle=shared;
    self->gl.create_memory(1,&texture->memory);const GLint dedicated=GL_TRUE;
    self->gl.memory_parameter(texture->memory,memory_dedicated,&dedicated);self->gl.import_memory(texture->memory,0,d3d12_resource,shared);
    const auto imported=glGetError();
    if(imported!=GL_NO_ERROR) {failure(*self,"D3D12 memory import",imported);self->gl.delete_memory(1,&texture->memory);return RSF_BACKEND_ERROR_FEATURE_FAILED;}
    GLint previous=0;glGetIntegerv(GL_TEXTURE_BINDING_2D,&previous);
    glGenTextures(1,&texture->texture);glBindTexture(GL_TEXTURE_2D,texture->texture);
    self->gl.texture_storage(GL_TEXTURE_2D,1,internal,static_cast<GLsizei>(width),static_cast<GLsizei>(height),texture->memory,0);
    glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_MIN_FILTER,GL_NEAREST);glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_MAG_FILTER,GL_NEAREST);
    glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_WRAP_S,0x812f);glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_WRAP_T,0x812f);
    glBindTexture(GL_TEXTURE_2D,static_cast<GLuint>(previous));
    const auto stored=glGetError();
    if(stored!=GL_NO_ERROR) {failure(*self,"shared texture storage",stored);glDeleteTextures(1,&texture->texture);self->gl.delete_memory(1,&texture->memory);return RSF_BACKEND_ERROR_FEATURE_FAILED;}
    *out=texture.release();return RSF_BACKEND_OK;
} catch(...) {return RSF_BACKEND_ERROR_FEATURE_FAILED;}
extern "C" rsf_backend_result rsf_gl_texture_get(rsf_gl_texture* self,rsf_gl_texture_info* info) {
    if(!self||!info||info->struct_size<sizeof(*info))return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    *info={sizeof(*info),self->texture,self->width,self->height,self->format,self->resource.Get()};return RSF_BACKEND_OK;
}
extern "C" rsf_backend_result rsf_gl_begin(rsf_gl_context* self,uint32_t count,rsf_gl_texture* const* textures,void** list,uint32_t* command_slot) {
    if(!owner(self)||!list||!command_slot||self->recording)return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    *list=nullptr;std::array<GLuint,16> names{};std::array<GLenum,16> layouts{};
    if(!barriers(*self,count,textures,names,layouts))return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    auto& slot=self->slots[self->next_slot];
    if(!wait_cpu(*self,slot.completion)||FAILED(slot.allocator->Reset())||FAILED(slot.list->Reset(slot.allocator.Get(),nullptr)))return RSF_BACKEND_ERROR_FEATURE_FAILED;
    const uint64_t ready=++self->serial;self->gl.semaphore_parameter(self->semaphore,fence_value,&ready);
    self->gl.signal(self->semaphore,0,nullptr,count,names.data(),layouts.data());glFlush();
    if(FAILED(self->queue->Wait(self->fence.Get(),ready))) {slot.list->Close();return RSF_BACKEND_ERROR_FEATURE_FAILED;}
    self->recording=true;self->current_slot=self->next_slot;*list=slot.list.Get();*command_slot=self->current_slot;return RSF_BACKEND_OK;
}
extern "C" rsf_backend_result rsf_gl_submit(rsf_gl_context* self,uint32_t count,rsf_gl_texture* const* textures) {
    if(!owner(self)||!self->recording)return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    std::array<GLuint,16> names{};std::array<GLenum,16> layouts{};
    if(!barriers(*self,count,textures,names,layouts))return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    auto& slot=self->slots[self->current_slot];if(FAILED(slot.list->Close()))return RSF_BACKEND_ERROR_FEATURE_FAILED;
    ID3D12CommandList* lists[]{slot.list.Get()};self->queue->ExecuteCommandLists(1,lists);
    const uint64_t complete=++self->serial;if(FAILED(self->queue->Signal(self->fence.Get(),complete)))return RSF_BACKEND_ERROR_FEATURE_FAILED;
    self->gl.semaphore_parameter(self->semaphore,fence_value,&complete);self->gl.wait(self->semaphore,0,nullptr,count,names.data(),layouts.data());
    slot.completion=complete;self->recording=false;self->next_slot=(self->current_slot+1)%3;return RSF_BACKEND_OK;
}
extern "C" rsf_backend_result rsf_gl_drain(rsf_gl_context* self) {
    if(!owner(self)||self->recording)return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    glFinish();const uint64_t complete=++self->serial;
    return SUCCEEDED(self->queue->Signal(self->fence.Get(),complete))&&wait_cpu(*self,complete)?RSF_BACKEND_OK:RSF_BACKEND_ERROR_FEATURE_FAILED;
}
extern "C" void rsf_gl_texture_destroy(rsf_gl_texture* self) {
    if(!self||!owner(self->owner))return;
    glDeleteTextures(1,&self->texture);self->owner->gl.delete_memory(1,&self->memory);delete self;
}
extern "C" rsf_backend_result rsf_gl_close(rsf_gl_context* self) {
    if(!self)return RSF_BACKEND_OK;
    const auto result=rsf_gl_drain(self);if(result!=RSF_BACKEND_OK)return result;
    self->gl.delete_semaphore(1,&self->semaphore);CloseHandle(self->fence_handle);CloseHandle(self->event);delete self;return RSF_BACKEND_OK;
}

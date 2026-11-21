// SPDX-License-Identifier: MIT
// WGL, OpenGL, DXGI and D3D12 calls for the Rust sharing owner in gl_shared_surface.rs. Each leaf
// performs one platform operation. Owner admission, format validation, semaphore, handle and
// resource lifetime, slot and fence synchronization, queue handoff, recovery and teardown are
// decided in Rust, which also implements the remaining rsf_gl_* entry points. GL leaves return
// glGetError after their call; created COM interfaces carry one reference for the caller.
#include <rescaleframe/gl_shared_surface.h>
#include "../../backends/common/d3d12_helpers.h"
#include <windows.h>
#include <GL/gl.h>
#include <d3d12.h>
#include <dxgi1_4.h>
#include <wrl/client.h>
#include <cstdint>
#include <cstring>
using Microsoft::WRL::ComPtr;

// Values gl_shared_surface.rs names directly.
static_assert(DXGI_FORMAT_R16G16B16A16_FLOAT == 10 && DXGI_FORMAT_R16G16_FLOAT == 34 && DXGI_FORMAT_R32_FLOAT == 41 &&
              DXGI_FORMAT_R8G8B8A8_UNORM == 28);
static_assert(sizeof(GLuint) == sizeof(uint32_t) && sizeof(GLenum) == sizeof(uint32_t) && sizeof(LUID) == 8);
static_assert(sizeof(HRESULT) == sizeof(int32_t));

namespace {
constexpr GLenum luid_query = 0x9599, memory_dedicated = 0x9581, d3d12_resource = 0x958a;
constexpr GLenum d3d12_fence = 0x9594, fence_value = 0x9595;

// Storage is `GlProcs` in gl_shared_surface.rs.
struct Gl {
    const GLubyte*(APIENTRY* string_index)(GLenum, GLuint);
    void(APIENTRY* luid)(GLenum, GLubyte*);
    void(APIENTRY* create_memory)(GLsizei, GLuint*);
    void(APIENTRY* delete_memory)(GLsizei, const GLuint*);
    void(APIENTRY* memory_parameter)(GLuint, GLenum, const GLint*);
    void(APIENTRY* import_memory)(GLuint, uint64_t, GLenum, void*);
    void(APIENTRY* texture_storage)(GLenum, GLsizei, GLenum, GLsizei, GLsizei, GLuint, uint64_t);
    void(APIENTRY* create_semaphore)(GLsizei, GLuint*);
    void(APIENTRY* delete_semaphore)(GLsizei, const GLuint*);
    void(APIENTRY* import_semaphore)(GLuint, GLenum, void*);
    void(APIENTRY* semaphore_parameter)(GLuint, GLenum, const uint64_t*);
    void(APIENTRY* signal)(GLuint, GLuint, const GLuint*, GLuint, const GLuint*, const GLenum*);
    void(APIENTRY* wait)(GLuint, GLuint, const GLuint*, GLuint, const GLuint*, const GLenum*);
};
static_assert(sizeof(Gl) == 13 * sizeof(void*));

template<class T> bool resolve(T& target, const char* name)
{
    target = reinterpret_cast<T>(wglGetProcAddress(name));
    return target && reinterpret_cast<uintptr_t>(target) > 3 && reinterpret_cast<intptr_t>(target) != -1;
}

bool extension(const Gl& gl, const char* name)
{
    GLint count = 0;
    glGetIntegerv(0x821d, &count);
    for (GLint i = 0; i < count; ++i) {
        const auto* value = gl.string_index(GL_EXTENSIONS, static_cast<GLuint>(i));
        if (value && std::strcmp(reinterpret_cast<const char*>(value), name) == 0) return true;
    }
    return false;
}

void* gl_current() { return wglGetCurrentContext(); }

// Resolves the entry points and requires both Win32 interop extensions.
int gl_load(Gl* gl)
{
    *gl = {};
    return resolve(gl->string_index, "glGetStringi") && resolve(gl->luid, "glGetUnsignedBytevEXT") &&
           resolve(gl->create_memory, "glCreateMemoryObjectsEXT") &&
           resolve(gl->delete_memory, "glDeleteMemoryObjectsEXT") &&
           resolve(gl->memory_parameter, "glMemoryObjectParameterivEXT") &&
           resolve(gl->import_memory, "glImportMemoryWin32HandleEXT") &&
           resolve(gl->texture_storage, "glTexStorageMem2DEXT") && resolve(gl->create_semaphore, "glGenSemaphoresEXT") &&
           resolve(gl->delete_semaphore, "glDeleteSemaphoresEXT") &&
           resolve(gl->import_semaphore, "glImportSemaphoreWin32HandleEXT") &&
           resolve(gl->semaphore_parameter, "glSemaphoreParameterui64vEXT") &&
           resolve(gl->signal, "glSignalSemaphoreEXT") && resolve(gl->wait, "glWaitSemaphoreEXT") &&
           extension(*gl, "GL_EXT_memory_object_win32") && extension(*gl, "GL_EXT_semaphore_win32");
}

uint32_t gl_error() { return glGetError(); }
void gl_luid(const Gl* gl, uint8_t* luid) { gl->luid(luid_query, luid); }

uint32_t semaphore_import(const Gl* gl, void* handle, uint32_t* name)
{
    gl->create_semaphore(1, name);
    gl->import_semaphore(*name, d3d12_fence, handle);
    return glGetError();
}
void semaphore_delete(const Gl* gl, uint32_t name) { gl->delete_semaphore(1, &name); }

uint32_t memory_import(const Gl* gl, void* handle, uint32_t* memory)
{
    gl->create_memory(1, memory);
    const GLint dedicated = GL_TRUE;
    gl->memory_parameter(*memory, memory_dedicated, &dedicated);
    gl->import_memory(*memory, 0, d3d12_resource, handle);
    return glGetError();
}
void memory_delete(const Gl* gl, uint32_t memory) { gl->delete_memory(1, &memory); }

// Creates a nearest-filtered, edge-clamped texture over the memory and restores the 2D binding.
uint32_t texture_store(const Gl* gl, uint32_t memory, uint32_t internal, uint32_t width, uint32_t height,
                       uint32_t* texture)
{
    GLint previous = 0;
    glGetIntegerv(GL_TEXTURE_BINDING_2D, &previous);
    glGenTextures(1, texture);
    glBindTexture(GL_TEXTURE_2D, *texture);
    gl->texture_storage(GL_TEXTURE_2D, 1, internal, static_cast<GLsizei>(width), static_cast<GLsizei>(height), memory, 0);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_NEAREST);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_NEAREST);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_S, 0x812f);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_T, 0x812f);
    glBindTexture(GL_TEXTURE_2D, static_cast<GLuint>(previous));
    return glGetError();
}
void texture_delete(uint32_t texture) { glDeleteTextures(1, &texture); }

// Signals `value` after GL's prior work, releasing the textures, and flushes so the queue sees it.
void release(const Gl* gl, uint32_t semaphore, uint64_t value, uint32_t count, const uint32_t* textures,
             const uint32_t* layouts)
{
    gl->semaphore_parameter(semaphore, fence_value, &value);
    gl->signal(semaphore, 0, nullptr, count, textures, layouts);
    glFlush();
}
// Makes later GL work wait for `value` and reacquire the textures.
void acquire(const Gl* gl, uint32_t semaphore, uint64_t value, uint32_t count, const uint32_t* textures,
             const uint32_t* layouts)
{
    gl->semaphore_parameter(semaphore, fence_value, &value);
    gl->wait(semaphore, 0, nullptr, count, textures, layouts);
}
void finish() { glFinish(); }

int32_t device_create(const uint8_t* luid, void** out)
{
    *out = nullptr;
    LUID adapter_luid{};
    std::memcpy(&adapter_luid, luid, sizeof(adapter_luid));
    ComPtr<IDXGIFactory4> factory;
    ComPtr<IDXGIAdapter1> adapter;
    ComPtr<ID3D12Device> device;
    auto result = CreateDXGIFactory1(IID_PPV_ARGS(&factory));
    if (SUCCEEDED(result)) result = factory->EnumAdapterByLuid(adapter_luid, IID_PPV_ARGS(&adapter));
    if (SUCCEEDED(result)) result = D3D12CreateDevice(adapter.Get(), D3D_FEATURE_LEVEL_11_0, IID_PPV_ARGS(&device));
    if (SUCCEEDED(result)) *out = device.Detach();
    return result;
}
int32_t queue_create(void* device, void** out)
{
    *out = nullptr;
    D3D12_COMMAND_QUEUE_DESC desc{};
    desc.Type = D3D12_COMMAND_LIST_TYPE_DIRECT;
    return static_cast<ID3D12Device*>(device)->CreateCommandQueue(&desc, __uuidof(ID3D12CommandQueue), out);
}
void device_luid(void* device, uint8_t* out)
{
    const auto luid = static_cast<ID3D12Device*>(device)->GetAdapterLuid();
    std::memcpy(out, &luid, sizeof(luid));
}
int32_t queue_device(void* queue, void** out)
{
    *out = nullptr;
    return static_cast<ID3D12CommandQueue*>(queue)->GetDevice(__uuidof(ID3D12Device), out);
}
int32_t fence_create(void* device, void** out)
{
    *out = nullptr;
    return static_cast<ID3D12Device*>(device)->CreateFence(0, D3D12_FENCE_FLAG_SHARED, __uuidof(ID3D12Fence), out);
}
// `object` is a fence or a committed resource.
int32_t shared_handle(void* device, void* object, void** out)
{
    HANDLE shared = nullptr;
    const auto result = static_cast<ID3D12Device*>(device)->CreateSharedHandle(
        static_cast<ID3D12DeviceChild*>(object), nullptr, GENERIC_ALL, nullptr, &shared);
    *out = SUCCEEDED(result) ? shared : nullptr;
    return result;
}
void* event_create() { return CreateEventW(nullptr, FALSE, FALSE, nullptr); }
void handle_close(void* handle) { CloseHandle(handle); }

// One direct allocator with a closed list recorded from it.
int32_t slot_create(void* device, void** allocator, void** list)
{
    *allocator = *list = nullptr;
    const auto owner = static_cast<ID3D12Device*>(device);
    ComPtr<ID3D12CommandAllocator> commands;
    ComPtr<ID3D12GraphicsCommandList> recorded;
    auto result = owner->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT, IID_PPV_ARGS(&commands));
    if (SUCCEEDED(result))
        result = owner->CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, commands.Get(), nullptr, IID_PPV_ARGS(&recorded));
    if (SUCCEEDED(result)) result = recorded->Close();
    if (SUCCEEDED(result)) {
        *allocator = commands.Detach();
        *list = recorded.Detach();
    }
    return result;
}
int32_t slot_reset(void* allocator, void* list)
{
    const auto commands = static_cast<ID3D12CommandAllocator*>(allocator);
    auto result = commands->Reset();
    if (SUCCEEDED(result)) result = static_cast<ID3D12GraphicsCommandList*>(list)->Reset(commands, nullptr);
    return result;
}
int32_t list_close(void* list) { return static_cast<ID3D12GraphicsCommandList*>(list)->Close(); }

uint64_t fence_completed(void* fence) { return static_cast<ID3D12Fence*>(fence)->GetCompletedValue(); }
int fence_wait(void* fence, uint64_t value, void* event, uint32_t milliseconds)
{
    return SUCCEEDED(rsf::wait_fence(static_cast<ID3D12Fence*>(fence), value, event, milliseconds));
}
int32_t queue_wait(void* queue, void* fence, uint64_t value)
{
    return static_cast<ID3D12CommandQueue*>(queue)->Wait(static_cast<ID3D12Fence*>(fence), value);
}
void queue_execute(void* queue, void* list)
{
    ID3D12CommandList* lists[]{static_cast<ID3D12GraphicsCommandList*>(list)};
    static_cast<ID3D12CommandQueue*>(queue)->ExecuteCommandLists(1, lists);
}
int32_t queue_signal(void* queue, void* fence, uint64_t value)
{
    return static_cast<ID3D12CommandQueue*>(queue)->Signal(static_cast<ID3D12Fence*>(fence), value);
}

// A shared, simultaneous-access 2D texture usable as render target and UAV, created in COMMON.
int32_t resource_create(void* device, uint32_t width, uint32_t height, uint32_t format, void** out)
{
    *out = nullptr;
    D3D12_RESOURCE_DESC desc{};
    desc.Dimension = D3D12_RESOURCE_DIMENSION_TEXTURE2D;
    desc.Width = width;
    desc.Height = height;
    desc.DepthOrArraySize = desc.MipLevels = 1;
    desc.Format = static_cast<DXGI_FORMAT>(format);
    desc.SampleDesc.Count = 1;
    desc.Flags = D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET | D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS |
                 D3D12_RESOURCE_FLAG_ALLOW_SIMULTANEOUS_ACCESS;
    D3D12_HEAP_PROPERTIES heap{};
    heap.Type = D3D12_HEAP_TYPE_DEFAULT;
    heap.CreationNodeMask = heap.VisibleNodeMask = 1;
    return static_cast<ID3D12Device*>(device)->CreateCommittedResource(
        &heap, D3D12_HEAP_FLAG_SHARED, &desc, D3D12_RESOURCE_STATE_COMMON, nullptr, __uuidof(ID3D12Resource), out);
}

// Field order matches `Leaves` in gl_shared_surface.rs.
struct Leaves {
    decltype(&gl_current) gl_current;
    decltype(&gl_load) gl_load;
    decltype(&gl_error) gl_error;
    decltype(&gl_luid) gl_luid;
    decltype(&semaphore_import) semaphore_import;
    decltype(&semaphore_delete) semaphore_delete;
    decltype(&memory_import) memory_import;
    decltype(&memory_delete) memory_delete;
    decltype(&texture_store) texture_store;
    decltype(&texture_delete) texture_delete;
    decltype(&release) release;
    decltype(&acquire) acquire;
    decltype(&finish) finish;
    decltype(&device_create) device_create;
    decltype(&queue_create) queue_create;
    decltype(&device_luid) device_luid;
    decltype(&queue_device) queue_device;
    decltype(&fence_create) fence_create;
    decltype(&shared_handle) shared_handle;
    decltype(&event_create) event_create;
    decltype(&handle_close) handle_close;
    decltype(&slot_create) slot_create;
    decltype(&slot_reset) slot_reset;
    decltype(&list_close) list_close;
    decltype(&fence_completed) fence_completed;
    decltype(&fence_wait) fence_wait;
    decltype(&queue_wait) queue_wait;
    decltype(&queue_execute) queue_execute;
    decltype(&queue_signal) queue_signal;
    decltype(&resource_create) resource_create;
};

constexpr Leaves leaves{
    gl_current,   gl_load,      gl_error,       gl_luid,      semaphore_import, semaphore_delete,
    memory_import, memory_delete, texture_store, texture_delete, release,        acquire,
    finish,       device_create, queue_create,  device_luid,  queue_device,     fence_create,
    shared_handle, event_create, handle_close,  slot_create,  slot_reset,       list_close,
    fence_completed, fence_wait, queue_wait,    queue_execute, queue_signal,    resource_create,
};
}

extern "C" rsf_backend_result rsf_core_gl_open(const rsf_gl_setup*, rsf_gl_context**, const Leaves*);

// The leaf table enters here; the other rsf_gl_* entry points are defined in Rust.
extern "C" rsf_backend_result rsf_gl_open(const rsf_gl_setup* setup, rsf_gl_context** out)
{
    return rsf_core_gl_open(setup, out, &leaves);
}

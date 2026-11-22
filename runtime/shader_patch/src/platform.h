/* SPDX-License-Identifier: MIT */
/* Native leaves of the Runtime shader patch service.

   The Rust owner in runtime/orchestrator/src/shader_patch.rs decides everything: pack
   admission, source claims, ordered transformations, original proof, reflection policy,
   cache identity and files, counters, the creation inventory and hook lifetime. Each entry
   here is one yyjson, D3DCompiler, D3D11 COM or MinHook call with no policy of its own.
   Private to platform.cpp and its Rust owner; the public API stays in shader_patch.h. */

#ifndef RSF_SHADER_PATCH_PLATFORM_H
#define RSF_SHADER_PATCH_PLATFORM_H

#include <rescaleframe/shader_patch.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Receives bytes that are valid only for the duration of the call. */
typedef void (*rsf_patch_sink)(void* user, const void* data, size_t size);

/* Parsed JSON values in document order. An array or object reports its member count in
   `size`; each object member is a STRING key followed by its value. Integers report their
   64-bit pattern in `number`. Nulls, booleans and reals carry nothing the owner reads. */
enum {
    RSF_PATCH_JSON_OTHER = 0,
    RSF_PATCH_JSON_UINT = 1,
    RSF_PATCH_JSON_SINT = 2,
    RSF_PATCH_JSON_STRING = 3,
    RSF_PATCH_JSON_ARRAY = 4,
    RSF_PATCH_JSON_OBJECT = 5
};
typedef void (*rsf_patch_json_visit)(void* user, uint32_t kind, uint64_t number, const char* text,
                                     size_t size);

int32_t rsf_patch_parse_json(rsf_patch_bytes text, void* user, rsf_patch_json_visit visit);

/* Returns 0 when D3DCompile's include should receive these bytes, borrowed for the compile. */
typedef int32_t (*rsf_patch_include_fn)(void* user, const char* name, const void** data,
                                        size_t* size);

typedef struct rsf_patch_compile {
    void* compiler; /* D3DCompile-compatible entry point */
    rsf_patch_bytes source;
    const rsf_patch_define* defines;
    size_t define_count;
    const char* entry;
    const char* target;
    uint32_t flags;
    void* user;
    rsf_patch_include_fn include;
    rsf_patch_sink code;   /* called when D3DCompile returned a code blob */
    rsf_patch_sink errors; /* called when D3DCompile returned an error blob */
} rsf_patch_compile;

/* ID3D11ShaderReflection facts, borrowed for the duration of each callback. */
typedef struct rsf_patch_binding {
    const char* name;
    uint32_t type;
    uint32_t point;
    uint32_t count;
    uint32_t return_type;
    uint32_t dimension;
} rsf_patch_binding;

typedef struct rsf_patch_parameter {
    const char* semantic;
    uint32_t index;
    uint32_t reg;
    uint32_t system_value;
    uint32_t component_type;
    uint32_t mask;
} rsf_patch_parameter;

enum { RSF_PATCH_SIGNATURE_INPUT = 0, RSF_PATCH_SIGNATURE_OUTPUT = 1, RSF_PATCH_SIGNATURE_PATCH = 2 };

typedef struct rsf_patch_reflection {
    void* user;
    void (*shader)(void* user, uint32_t version);
    void (*binding)(void* user, const rsf_patch_binding* binding);
    void (*buffer)(void* user, const char* name, uint32_t size);
    /* A variable of the most recently reported buffer. */
    void (*variable)(void* user, const char* name, uint32_t offset, uint32_t size);
    void (*parameter)(void* user, uint32_t signature, const rsf_patch_parameter* parameter);
} rsf_patch_reflection;

/* d3d11.h values the owner compares, checked against the SDK in platform.cpp. */
enum {
    RSF_PATCH_USAGE_DYNAMIC = 2,
    RSF_PATCH_CPU_ACCESS_WRITE = 0x10000,
    RSF_PATCH_CONSTANT_SLOTS = 14
};

typedef struct rsf_patch_platform {
    uint32_t struct_size;
    /* yyjson_read with no flags. Returns 0 after visiting the whole document. */
    int32_t (*parse_json)(rsf_patch_bytes text, void* user, rsf_patch_json_visit visit);
    /* Returns the D3DCompile HRESULT. */
    int32_t (*compile)(const rsf_patch_compile* call);
    /* D3DReflect. Returns 0 after reporting every fact. */
    int32_t (*reflect)(rsf_patch_bytes code, const rsf_patch_reflection* sink);
    /* D3DDisassemble with instruction numbering. Returns 0 after writing the text. */
    int32_t (*disassemble)(rsf_patch_bytes code, void* user, rsf_patch_sink text);
    /* ID3D11Device::Create<Stage>Shader for v, p, g, h, d or c. Returns the HRESULT. */
    int32_t (*create_shader)(void* device, uint32_t stage, rsf_patch_bytes code, void** shader);
    /* A dynamic, CPU-writable constant buffer. Returns the HRESULT. */
    int32_t (*create_constant_buffer)(void* device, uint32_t size, void** buffer);
    void (*buffer_desc)(void* buffer, uint32_t* width, uint32_t* usage, uint32_t* cpu_access);
    /* Map with WRITE_DISCARD, copy and unmap. Returns 0 when written. */
    int32_t (*upload)(void* context, void* buffer, rsf_patch_bytes data);
    /* <Stage>SetConstantBuffers for one slot of v, p, g, h, d or c. */
    void (*bind_constant)(void* context, uint32_t stage, uint32_t slot, void* buffer);
    /* MH_Initialize. Returns 1 when MinHook is initialized, including by an earlier call. */
    int32_t (*hooks_ready)(void);
    /* MH_CreateHook and MH_EnableHook. Return 1 on success. */
    int32_t (*hook_create)(void* target, void* detour, void** original);
    int32_t (*hook_enable)(void* target);
    void (*hook_disable)(void* target);
    /* MH_RemoveHook for a hook that has never been enabled. */
    void (*hook_remove)(void* target);
} rsf_patch_platform;

/* Development entry points for the corpus parity and synthetic fixtures. They are linked
   into the Runtime but not exported. `reason` receives a refusal and holds 256 bytes. */
typedef void (*rsf_patch_file_sink)(void* user, const char* name, size_t name_size,
                                    const void* data, size_t size);
int32_t rsf_patch_transform(rsf_patch_bytes pack, const rsf_patch_bytes* names,
                            const rsf_patch_bytes* files, size_t count, void* user,
                            rsf_patch_file_sink output, char* reason);
int32_t rsf_patch_rewrite_dxbc(rsf_patch_bytes input, const char* expected_sha256,
                               const uint32_t* instruction, size_t instruction_words,
                               const uint32_t* replacement, size_t replacement_words, void* user,
                               rsf_patch_sink output, char* reason);

#ifdef __cplusplus
}
#endif

#endif

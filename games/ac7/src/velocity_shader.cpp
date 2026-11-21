// SPDX-License-Identifier: MIT
#include "velocity_shader.h"
#include "shader_bytes.h"
#include "velocity_container.h"
#include "velocity_tokens.h"
#include <rescaleframe/shader_compile.h>
#include <algorithm>
#include <array>
#include <cstring>
#include <string>
#include <map>
#include <new>
#include <atomic>

namespace {
std::atomic<uint32_t> geometry_compiles{0};
struct Chunk { std::array<uint8_t,4> tag{}; std::vector<uint8_t> bytes; };
struct Container {
    std::vector<Chunk> chunks;
    bool read(const void* pointer, size_t size) {
        return rsf_core_ac7_velocity_read_container(pointer, size, &chunks,
            [](void* user, const rsf_ac7_velocity_chunk_view* view) noexcept {
                try {
                    auto& stored = *static_cast<std::vector<Chunk>*>(user);
                    Chunk chunk{{view->tag[0], view->tag[1], view->tag[2], view->tag[3]}, {}};
                    chunk.bytes.assign(view->bytes, view->bytes + view->size);
                    stored.push_back(std::move(chunk));
                    return true;
                } catch (...) { return false; }
            });
    }
    Chunk* find(const char* tag) {
        for (auto& chunk:chunks) if (!std::memcmp(chunk.tag.data(),tag,4)) return &chunk;
        return nullptr;
    }
    void write(std::vector<uint8_t>& output) {
        std::vector<rsf_ac7_velocity_chunk_view> views;
        views.reserve(chunks.size());
        for (const auto& chunk : chunks)
            views.push_back({{chunk.tag[0], chunk.tag[1], chunk.tag[2], chunk.tag[3]},
                chunk.bytes.data(), chunk.bytes.size()});
        if (!rsf_core_ac7_velocity_write_container(views.data(), views.size(), &output, rsf::ac7::resize_shader_bytes))
            throw std::bad_alloc{};
    }
};

using Semantic=rsf_ac7_velocity_semantic;
bool append_semantic(void* user, const rsf_ac7_velocity_semantic_view* view) noexcept
try {
    auto& semantics = *static_cast<std::vector<Semantic>*>(user);
    semantics.push_back({std::string(view->name, view->name_size),
        view->index, view->system, view->type, view->reg, view->mask, view->used});
    return true;
} catch (...) { return false; }

bool read_signature(const Chunk& chunk, std::vector<Semantic>& result)
{
    const bool streams = std::memcmp(chunk.tag.data(), "OSG5", 4) == 0;
    return rsf_core_ac7_velocity_read_signature(chunk.bytes.data(), chunk.bytes.size(), streams, &result,
        append_semantic);
}

std::vector<rsf_ac7_velocity_semantic_view> semantic_views(const std::vector<Semantic>& semantics)
{
    std::vector<rsf_ac7_velocity_semantic_view> views;
    views.reserve(semantics.size());
    for (const auto& semantic : semantics)
        views.push_back({semantic.name.data(), semantic.name.size(), semantic.index,
            semantic.system, semantic.type, semantic.reg, semantic.mask, semantic.used});
    return views;
}

void write_signature(Chunk& chunk, const std::vector<Semantic>& semantics)
{
    const bool streams = std::memcmp(chunk.tag.data(), "OSG5", 4) == 0;
    const auto views = semantic_views(semantics);
    if (!rsf_core_ac7_velocity_write_signature(views.data(), views.size(), streams, &chunk.bytes,
            rsf::ac7::resize_shader_bytes))
        throw std::bad_alloc{};
}

using Operand = rsf_ac7_velocity_operand_view;
using OperandEdit = rsf_ac7_velocity_operand_edit;

struct Instruction {
    std::vector<uint32_t> words;
    uint32_t op;
    bool declaration;

    bool operands(std::vector<Operand>& result) const {
        return rsf_core_ac7_velocity_read_operands(words.data(), words.size(), &result,
            [](void* user, const Operand* view) noexcept {
                try {
                    static_cast<std::vector<Operand>*>(user)->push_back(*view);
                    return true;
                } catch (...) { return false; }
            });
    }

    template<class Select>
    bool rewrite(Select select, bool nested = false) {
        struct Context {
            Select select;
            std::vector<uint32_t> rewritten;
        } context{select, {}};
        if (!rsf_core_ac7_velocity_rewrite_operands(words.data(), words.size(), nested, &context,
                [](void* user, const Operand* operand, OperandEdit* edit) noexcept {
                    try { return static_cast<Context*>(user)->select(*operand, *edit); }
                    catch (...) { return false; }
                },
                [](void* user, const uint32_t* data, size_t count) noexcept {
                    try {
                        static_cast<Context*>(user)->rewritten.assign(data, data + count);
                        return true;
                    } catch (...) { return false; }
                }))
            return false;
        if (!context.rewritten.empty()) words.swap(context.rewritten);
        return true;
    }
};

bool instructions(const Chunk& chunk, std::vector<Instruction>& output, uint32_t profile = 0x50)
{
    return rsf_core_ac7_velocity_read_instructions(chunk.bytes.data(), chunk.bytes.size(), profile, &output,
        [](void* user, const rsf_ac7_velocity_instruction_view* view) noexcept {
            try {
                auto& result = *static_cast<std::vector<Instruction>*>(user);
                result.push_back({std::vector<uint32_t>(view->words, view->words + view->count),
                    view->opcode, view->declaration});
                return true;
            } catch (...) { return false; }
        });
}

void write_instructions(Chunk& chunk, const std::vector<uint32_t>& code)
{
    if (!rsf_core_ac7_velocity_write_instructions(code.data(), code.size(), &chunk.bytes,
            rsf::ac7::resize_shader_bytes))
        throw std::bad_alloc{};
}

bool rebase_temp(const Operand& operand, OperandEdit& edit, uint32_t base)
{
    if (!operand.direct(1) || operand.reg() >= 4096 || base + operand.reg() >= 4096)
        return false;
    edit.remap(operand, base + operand.reg());
    return true;
}

uint32_t temp_count(const std::vector<Instruction>& code)
{ for (const auto& i:code) if (i.op==104) return i.words[1]; return 0; }
bool has_cb(const std::vector<Instruction>& code,uint32_t slot)
{
    for (const auto& i:code) if (i.op==89 && i.words.size()>2 && i.words[2]==slot) return true;
    return false;
}
// D3DCompiler's blobs and module remain native. The cached copy owns the returned
// bytecode for the lifetime of this module, including throughout every Rust call.
bool compile_pixel_helper(const char* source, size_t length, const uint8_t** bytes, size_t* size) noexcept
try {
    static const auto bytecode = [source, length] {
        std::vector<uint8_t> result;
        rsf::compile_shader(source, length, "ac7-translucent-velocity", "main", "ps_5_0", result);
        return result;
    }();
    *bytes = bytecode.data();
    *size = bytecode.size();
    return !bytecode.empty();
} catch (...) { return false; }

struct VertexShaderSource {
    const char* text;
    size_t length;
};
struct VertexMetadata {
    uint32_t particle;
    uint32_t position_register;
};
static_assert(sizeof(VertexShaderSource) == 16);
static_assert(sizeof(VertexMetadata) == 8);

bool compile_vertex_helper(const VertexShaderSource* sources, uint32_t particle,
    const uint8_t** bytes, size_t* size) noexcept
try {
    if (particle >= 4) return false;
    const auto compile = [sources](size_t index, const char* name) {
        const auto& source = sources[index];
        std::vector<uint8_t> result;
        rsf::compile_shader(source.text, source.length, name, "main", "vs_5_0", result);
        return result;
    };
    // Keep the original initialization order and lend bytecode owned by this module.
    static const auto generic = compile(0, "ac7-geometry-key");
    static const auto sprite = compile(1, "ac7-particle-key");
    static const auto mesh = compile(2, "ac7-mesh-particle-pose-key");
    static const auto beam = compile(3, "ac7-beam-sample-key");
    const std::vector<uint8_t>* const helpers[]{&generic, &sprite, &mesh, &beam};
    const auto& helper = *helpers[particle];
    *bytes = helper.data();
    *size = helper.size();
    return !helper.empty();
} catch (...) { return false; }
}

extern "C" bool rsf_core_ac7_velocity_pixel_shader(const uint8_t*, size_t, uint32_t blend,
    uint32_t previous, uint32_t current, bool (*compile_helper)(const char*, size_t, const uint8_t**, size_t*),
    void* user, uint8_t* (*resize)(void*, size_t)) noexcept;

extern "C" bool rsf_core_ac7_velocity_vertex_shader(const uint8_t*, size_t, bool tessellation, bool beam,
    bool (*compile_helper)(const VertexShaderSource*, uint32_t, const uint8_t**, size_t*), void* user,
    uint8_t* (*resize)(void*, size_t), bool (*append)(void*, const rsf_ac7_velocity_semantic_view*),
    VertexMetadata*) noexcept;

rsf_ac7_velocity_shader_result rsf_ac7_velocity_pixel_shader(const void* bytes, size_t size,
    uint32_t blend, std::vector<uint8_t>& output, uint32_t previous_register, uint32_t current_register) noexcept
{
    // The input may point into output. Keep it alive until Rust has finished borrowing it.
    std::vector<uint8_t> patched;
    if (rsf_core_ac7_velocity_pixel_shader(static_cast<const uint8_t*>(bytes), size, blend,
            previous_register, current_register, compile_pixel_helper, &patched, rsf::ac7::resize_shader_bytes)) {
        output.swap(patched);
        return rsf_ac7_velocity_shader_result::patched;
    }
    output.clear();
    return rsf_ac7_velocity_shader_result::refused;
}

bool rsf_ac7_velocity_signature(const void* bytes,size_t size,bool output,
    std::vector<rsf_ac7_velocity_semantic>& semantics) noexcept
{
    semantics.clear();
    try {
        Container container; if (!container.read(bytes,size)) return false;
        const auto* chunk=container.find(output ? "OSGN" : "ISGN");
        if (!chunk && output) chunk=container.find("OSG5");
        return chunk && read_signature(*chunk,semantics);
    } catch (...) { semantics.clear(); return false; }
}
rsf_ac7_velocity_shader_result rsf_ac7_velocity_vertex_shader(const void* bytes,size_t size,
    std::vector<uint8_t>& output,rsf_ac7_velocity_vertex_info& info,bool tessellation,bool beam) noexcept
{
    // Keep caller-owned input live even when it points into the destination vector.
    struct Destination {
        std::vector<uint8_t> bytes;
        rsf_ac7_velocity_vertex_info info;
    } patched;
    VertexMetadata metadata{};
    if (rsf_core_ac7_velocity_vertex_shader(static_cast<const uint8_t*>(bytes), size, tessellation,
            beam, compile_vertex_helper, &patched,
            [](void* user, size_t count) noexcept {
                return rsf::ac7::resize_shader_bytes(&static_cast<Destination*>(user)->bytes, count);
            },
            [](void* user, const rsf_ac7_velocity_semantic_view* view) noexcept {
                return append_semantic(&static_cast<Destination*>(user)->info.outputs, view);
            }, &metadata)) {
        patched.info.particle = metadata.particle;
        patched.info.position_register = metadata.position_register;
        output.swap(patched.bytes);
        info = std::move(patched.info);
        return rsf_ac7_velocity_shader_result::patched;
    }
    output.clear();
    info = {};
    return rsf_ac7_velocity_shader_result::refused;
}

namespace {
struct GeometryRegisters {
    uint32_t* previous;
    uint32_t* current;
    uint32_t* maximum_vertices;
};
static_assert(sizeof(GeometryRegisters) == 24);

using CompileGeometryHelper = bool (*)(const char*, size_t, const uint8_t**, size_t*);

// Each typed compiler leaf lends its immutable bytecode for the lifetime of the module.
bool compile_history_geometry(const char* source, size_t length, const uint8_t** bytes, size_t* size) noexcept
try {
    static const auto code = [source, length] {
        std::vector<uint8_t> bytecode;
        ++geometry_compiles;
        try {
            rsf::compile_shader(source, length, "ac7-previous-geometry", "main", "gs_5_0", bytecode);
        } catch (...) { bytecode.clear(); }
        return bytecode;
    }();
    *bytes = code.data();
    *size = code.size();
    return !code.empty();
} catch (...) { return false; }

bool compile_native_geometry_helper(const char* source, size_t length, const uint8_t** bytes, size_t* size) noexcept
try {
    static const auto code = [source, length] {
        std::vector<uint8_t> bytecode;
        rsf::compile_shader(source, length, "ac7-native-generated-geometry", "main", "vs_5_0", bytecode);
        return bytecode;
    }();
    *bytes = code.data();
    *size = code.size();
    return !code.empty();
} catch (...) { return false; }
}

extern "C" bool rsf_core_ac7_velocity_geometry_shader(const rsf_ac7_velocity_semantic_view*, size_t,
    CompileGeometryHelper, void* user, uint8_t* (*resize)(void*, size_t), const GeometryRegisters*) noexcept;

extern "C" bool rsf_core_ac7_velocity_native_geometry_shader(const uint8_t*, size_t,
    const rsf_ac7_velocity_semantic_view*, size_t, CompileGeometryHelper, void* user,
    uint8_t* (*resize)(void*, size_t), bool (*append)(void*, const rsf_ac7_velocity_semantic_view*),
    const GeometryRegisters*) noexcept;

bool rsf_ac7_velocity_geometry_shader(const rsf_ac7_velocity_vertex_info& info,
    std::vector<uint8_t>& bytecode, uint32_t& previous_register, uint32_t& current_register) noexcept
{
    bytecode.clear();
    previous_register = current_register = 0;
    try {
        const auto inputs = semantic_views(info.outputs);
        const GeometryRegisters registers{&previous_register, &current_register, nullptr};
        if (rsf_core_ac7_velocity_geometry_shader(inputs.data(), inputs.size(), compile_history_geometry,
                &bytecode, rsf::ac7::resize_shader_bytes, &registers))
            return true;
    } catch (...) {}
    bytecode.clear();
    return false;
}

uint32_t rsf_ac7_velocity_geometry_compile_count() noexcept { return geometry_compiles.load(); }

rsf_ac7_velocity_shader_result rsf_ac7_velocity_native_geometry_shader(const void* bytes, size_t size,
    const rsf_ac7_velocity_vertex_info& vertex, std::vector<uint8_t>& output,
    rsf_ac7_velocity_vertex_info& info, uint32_t& maximum_vertices,
    uint32_t& previous_register, uint32_t& current_register) noexcept
{
    maximum_vertices = 0;
    try {
        // The original DXBC and upstream semantics may live in the caller's destinations.
        struct Destination {
            std::vector<uint8_t> bytes;
            rsf_ac7_velocity_vertex_info info;
        } patched;
        const auto inputs = semantic_views(vertex.outputs);
        const GeometryRegisters registers{&previous_register, &current_register, &maximum_vertices};
        if (rsf_core_ac7_velocity_native_geometry_shader(static_cast<const uint8_t*>(bytes), size,
                inputs.data(), inputs.size(), compile_native_geometry_helper, &patched,
                [](void* user, size_t count) noexcept {
                    return rsf::ac7::resize_shader_bytes(&static_cast<Destination*>(user)->bytes, count);
                },
                [](void* user, const rsf_ac7_velocity_semantic_view* view) noexcept {
                    return append_semantic(&static_cast<Destination*>(user)->info.outputs, view);
                }, &registers)) {
            patched.info.particle = vertex.particle;
            output.swap(patched.bytes);
            info = std::move(patched.info);
            return rsf_ac7_velocity_shader_result::patched;
        }
    } catch (...) {}
    output.clear();
    info = {};
    return rsf_ac7_velocity_shader_result::refused;
}

rsf_ac7_velocity_shader_result rsf_ac7_velocity_hull_shader(const void* bytes,size_t size,
    const rsf_ac7_velocity_vertex_info& vertex,std::vector<uint8_t>& output,
    std::vector<uint8_t>& corners,rsf_ac7_velocity_vertex_info& info,uint32_t& input_points) noexcept
{
    output.clear();corners.clear();info={};input_points=0;
    try {
        Container native;if (!native.read(bytes,size)) return rsf_ac7_velocity_shader_result::refused;
        auto* shader=native.find("SHEX");if (!shader) shader=native.find("SHDR");
        auto* input=native.find("ISGN");auto* out=native.find("OSGN");auto* patch=native.find("PCSG");
        std::vector<Instruction> original;std::vector<Semantic> inputs,outputs,constants;
        if (!shader || !input || !out || !patch || !instructions(*shader,original,0x30050) ||
            !read_signature(*input,inputs) || !read_signature(*out,outputs) || !read_signature(*patch,constants))
            return rsf_ac7_velocity_shader_result::refused;
        uint32_t output_points=0,next=0;
        for (const auto& s:outputs) {
            if (s.name.starts_with("RSF")) return rsf_ac7_velocity_shader_result::refused;
            next=std::max(next,s.reg+1);
        }
        for (const auto& i:original) {
            if (i.op==147) input_points=(i.words[0]>>11)&63;
            if (i.op==148) output_points=(i.words[0]>>11)&63;
        }
        if (!input_points || input_points>32 || output_points!=3 || next+4>32)
            return rsf_ac7_velocity_shader_result::refused;
        std::vector<uint32_t> declarations,relay;
        auto append=[](auto& into,const auto& words){into.insert(into.end(),words.begin(),words.end());};
        uint32_t index_temp=0;bool control_temps=false,in_control=false;
        for (const auto& i:original) {
            if (i.op==114) in_control=true;
            if (i.op==115 || i.op==116) in_control=false;
            if (in_control && i.op==104 && i.words.size()==2) {index_temp=i.words[1];control_temps=true;}
        }
        if (index_temp>=4096) return rsf_ac7_velocity_shader_result::refused;
        append(relay,std::vector<uint32_t>{0x04000036u,0x00100012u,index_temp,0x00016001u});
        for (const char* name:{"RSFKEY","RSFLOOKUP","RSFCENTRE","RSFOLD"}) {
            const auto v=std::find_if(vertex.outputs.begin(),vertex.outputs.end(),[&](const auto& s){return s.name==name;});
            if (v==vertex.outputs.end() || std::any_of(inputs.begin(),inputs.end(),[&](const auto& s){return s.reg==v->reg;}))
                return rsf_ac7_velocity_shader_result::refused;
            auto in=*v;in.used=in.mask;inputs.push_back(in);auto mapped=in;mapped.reg=next++;outputs.push_back(mapped);
            append(declarations,std::vector<uint32_t>{0x0400005fu,0x002010f2u,input_points,v->reg,
                0x03000065u,0x001020f2u,mapped.reg});
            // Relative first index is the native output control-point ID. No sorting assumption.
            append(relay,std::vector<uint32_t>{0x07000036u,0x001020f2u,mapped.reg,0x00a01e46u,0x0010000au,index_temp,v->reg});
        }
        std::vector<uint32_t> ordinary{0x30050,0},low{0x30050,0};
        uint32_t phase=0;std::map<uint32_t,uint32_t> written;
        bool control_id=false;for (const auto& i:original) if (i.op==95 && i.words.size()==2 && ((i.words[1]>>12)&255)==22) control_id=true;
        bool explicit_control=std::any_of(original.begin(),original.end(),[](const auto& i){return i.op==114;});
        for (auto i:original) {
            if (!explicit_control && (i.op==115 || i.op==116)) {
                // Optimized identity hull shaders omit the control phase. Materialize it so
                // newly appended fields follow semantic identity rather than register order.
                if (input_points!=output_points) return rsf_ac7_velocity_shader_result::refused;
                std::vector<uint32_t> control{0x01000072u,0x0200005fu,0x00016000u,0x02000068u,1},copy;
                append(copy,std::vector<uint32_t>{0x04000036u,0x00100012u,0,0x00016001u});
                for (const auto& s:outputs) {
                    const auto in=std::find_if(inputs.begin(),inputs.end(),[&](const auto& v){return v.name==s.name && v.index==s.index;});
                    if (in==inputs.end() || in->mask!=s.mask) return rsf_ac7_velocity_shader_result::refused;
                    append(control,std::vector<uint32_t>{0x0400005fu,0x00201002u|(uint32_t(s.mask)<<4),input_points,in->reg,
                        0x03000065u,0x00102002u|(uint32_t(s.mask)<<4),s.reg});
                    append(copy,std::vector<uint32_t>{0x07000036u,0x00102002u|(uint32_t(s.mask)<<4),s.reg,0x00a01e46u,0x0010000au,0,in->reg});
                }
                append(control,copy);append(control,std::vector<uint32_t>{0x0100003eu});
                append(ordinary,control);append(low,control);explicit_control=true;
            }
            if (i.op==114 || i.op==115 || i.op==116) {phase=i.op;written.clear();}
            if (phase==114 && i.op==104) i.words[1]=index_temp+1;
            if (phase!=114 && i.op>=101 && i.op<=103 && i.words.size()>=3)
                written[i.words[2]]|=(i.words[1]>>4)&15;
            if (i.op==62 && phase==114) {append(ordinary,relay);append(low,relay);}
            if (i.op==62 && phase!=114) for (const auto& s:constants) {
                if (s.system<11 || s.system>16) continue;
                const uint32_t mask=s.mask&written[s.reg];if (!mask) continue;
                append(low,std::vector<uint32_t>{0x05000036u,0x00102002u|(mask<<4),s.reg,0x00004001u,0x3f800000u});
            }
            append(ordinary,i.words);
            if (i.op==150) {
                auto partition=i.words;partition[0]=(partition[0]&~0x00003800u)|0x00000800u;append(low,partition);
            } else append(low,i.words);
            if (i.op==114) {
                if (!control_temps) {append(ordinary,std::vector<uint32_t>{0x02000068u,1});append(low,std::vector<uint32_t>{0x02000068u,1});}
                if (!control_id) {append(ordinary,std::vector<uint32_t>{0x0200005fu,0x00016000u});append(low,std::vector<uint32_t>{0x0200005fu,0x00016000u});}
                append(ordinary,declarations);append(low,declarations);
            }
        }
        write_signature(*input,inputs);write_signature(*out,outputs);
        auto save=[&](const std::vector<uint32_t>& code,std::vector<uint8_t>& destination) {
            write_instructions(*shader,code);native.write(destination);
        };
        save(ordinary,output);save(low,corners);info.outputs=std::move(outputs);info.particle=vertex.particle;
        return rsf_ac7_velocity_shader_result::patched;
    } catch (...) {output.clear();corners.clear();info={};return rsf_ac7_velocity_shader_result::refused;}
}

rsf_ac7_velocity_shader_result rsf_ac7_velocity_domain_shader(const void* bytes,size_t size,
    const rsf_ac7_velocity_vertex_info& hull,std::vector<uint8_t>& output,
    rsf_ac7_velocity_vertex_info& info,uint32_t& previous_register,uint32_t& current_register) noexcept
{
    output.clear();info={};
    try {
        Container native,extra;if (!native.read(bytes,size)) return rsf_ac7_velocity_shader_result::refused;
        auto* shader=native.find("SHEX");if (!shader) shader=native.find("SHDR");
        auto* input=native.find("ISGN");auto* out=native.find("OSGN");
        std::vector<Instruction> original;std::vector<Semantic> inputs,outputs;
        if (!shader || !input || !out || !instructions(*shader,original,0x40050) ||
            !read_signature(*input,inputs) || !read_signature(*out,outputs) || has_cb(original,13))
            return rsf_ac7_velocity_shader_result::refused;
        uint32_t position=UINT32_MAX,next=0,points=0,returns=0;bool triangle=false,has_domain=false;
        for (const auto& s:outputs) {
            if (s.system==1 && s.mask==15 && s.type==3) position=s.reg;
            if (s.name.starts_with("RSF")) return rsf_ac7_velocity_shader_result::refused;
            next=std::max(next,s.reg+1);
        }
        for (const auto& i:original) {
            if (i.op==147) points=(i.words[0]>>11)&63;
            if (i.op==149) triangle=((i.words[0]>>11)&3)==2;
            if (i.op==95 && i.words.size()==2 && ((i.words[1]>>12)&255)==28) has_domain=true;
            if (i.op==62) ++returns;
            if (i.op==4 || i.op==5 || i.op==63 || ((i.op==88 || i.op==161 || i.op==162) && i.words.size()>2 && i.words[2]==127))
                return rsf_ac7_velocity_shader_result::refused;
        }
        if (!triangle || points!=3 || returns!=1 || original.back().op!=62 || position==UINT32_MAX || next+5>32)
            return rsf_ac7_velocity_shader_result::refused;
        static constexpr char source[]=R"(
ByteAddressBuffer History:register(t127);
cbuffer Parameters:register(b13) {
 float4 Rect;float4 Jitter;float Cutoff;uint HasHistory;uint HistoryMask;uint Padding;
 row_major float4x4 ClipToPrevious;
};
struct O {float4 key:RSFKEY0;float4 centre:RSFCENTRE0;float4 old:RSFOLD0;float4 current:RSFCUR0;float4 previous:RSFPREV0;};
uint3 patch_key(float4 a,float4 b,float4 c) {
 uint3 hash=uint3(2166136261,2246822519,3266489917);
 [unroll]for(uint v=0;v<3;v++) {
  uint4 bits=asuint(v==0 ? a : v==1 ? b : c);
  [unroll]for(uint j=0;j<4;j++)hash=(hash^bits[j])*uint3(16777619,3266489917,668265263);
 }
 return hash;
}
float4 previous_corner(float4 current,uint3 patch,uint corner,float3 old,bool sprite,bool valid) {
 float4 p=current;p.xy-=Jitter.xy*p.w;float4 fallback=mul(p,ClipToPrevious);fallback.xy+=Jitter.zw*fallback.w;
 if(HasHistory==0 || !valid)return fallback;
 uint4 key=uint4(patch,asuint(float(corner)));uint hash=2166136261;
 [unroll]for(uint j=0;j<4;j++)hash=(hash^key[j])*16777619;
 hash^=hash>>16;hash=max(1,hash&0x7fffffff);
 bool found=false,ambiguous=false;float distance=3.402823e38;float4 selected=fallback;
 [loop]for(uint probe=0;probe<32;probe++) {
  uint address=((hash+probe)&HistoryMask)*64;uint marker=History.Load(address);
  if(marker==0)break;if(marker!=hash || any(History.Load4(address+4)!=key))continue;
  float4 candidate=asfloat(History.Load4(address+20));if(!all(isfinite(candidate)) || candidate.w<=0)continue;
  float3 centre=asfloat(History.Load3(address+36));float d=sprite ? dot(centre-old,centre-old) : 0;
  if(!found || d<distance) {found=true;distance=d;selected=candidate;ambiguous=false;}
  else if(d==distance && any(asuint(candidate)!=asuint(selected)))ambiguous=true;
 }
 return found && !ambiguous ? selected : fallback;
}
O main(float3 domain:RSFDOMAIN0,float4 p0:RSFCORNER0,float4 p1:RSFCORNER1,float4 p2:RSFCORNER2,
 float4 a:RSFA0,float4 b:RSFA1,float4 c:RSFA2,float4 la:RSFB0,float4 lb:RSFB1,float4 lc:RSFB2,
 float4 centre:RSFCENTRE0,float4 old:RSFOLD0) {
 O o;uint3 now=patch_key(a,b,c),prior=patch_key(la,lb,lc);
 uint corner=domain.x==1 ? 1 : domain.y==1 ? 2 : domain.z==1 ? 3 : 0;
 o.key=asfloat(uint4(now,asuint(float(corner))));o.centre=centre;o.old=old;
 o.current=p0*domain.x+p1*domain.y+p2*domain.z;
 bool valid=la.w!=3 && lb.w!=3 && lc.w!=3;
 o.previous=previous_corner(p0,prior,1,old.xyz,la.w==1,valid)*domain.x+
  previous_corner(p1,prior,2,old.xyz,la.w==1,valid)*domain.y+
  previous_corner(p2,prior,3,old.xyz,la.w==1,valid)*domain.z;
 return o;
})";
        static const auto helper_bytes=[] {std::vector<uint8_t> code;
            rsf::compile_shader(source,"ac7-tessellated-control-motion","main","vs_5_0",code);return code;}();
        if (!extra.read(helper_bytes.data(),helper_bytes.size())) return rsf_ac7_velocity_shader_result::refused;
        auto* hs=extra.find("SHEX");if (!hs) hs=extra.find("SHDR");auto* hi=extra.find("ISGN");auto* ho=extra.find("OSGN");
        std::vector<Instruction> added;std::vector<Semantic> helper_inputs,helper_outputs;
        if (!hs || !hi || !ho || !instructions(*hs,added,0x10050) || !read_signature(*hi,helper_inputs) || !read_signature(*ho,helper_outputs))
            return rsf_ac7_velocity_shader_result::refused;
        const uint32_t base=temp_count(original),scratch=base*2,corner_base=scratch+32,domain=corner_base+3,current=domain+1,helper_base=current+1;
        const uint32_t total=helper_base+temp_count(added);if (total>4096) return rsf_ac7_velocity_shader_result::refused;
        struct Input {uint32_t kind,reg,point;};std::map<uint32_t,Input> mapping;std::map<uint32_t,uint32_t> output_map;
        std::vector<uint32_t> declarations,injection;
        std::vector<Instruction> body;
        auto append=[](auto& into,const auto& words){into.insert(into.end(),words.begin(),words.end());};
        if (!has_domain) append(declarations,std::vector<uint32_t>{0x0200005fu,0x0001c072u});
        for (const auto& s:helper_inputs) {
            if (s.name=="RSFDOMAIN") {mapping[s.reg]={28,0,0};continue;}
            if (s.name=="RSFCORNER") {mapping[s.reg]={0,corner_base+s.index,0};continue;}
            const char* name=s.name=="RSFA" ? "RSFKEY" : s.name=="RSFB" ? "RSFLOOKUP" : s.name.c_str();
            const auto found=std::find_if(hull.outputs.begin(),hull.outputs.end(),[&](const auto& v){return v.name==name;});
            if (found==hull.outputs.end()) return rsf_ac7_velocity_shader_result::refused;
            mapping[s.reg]={25,found->reg,s.name=="RSFA" || s.name=="RSFB" ? s.index : 0u};
            if (std::none_of(inputs.begin(),inputs.end(),[&](const auto& v){return v.reg==found->reg;})) {
                auto semantic=*found;semantic.used=semantic.mask;inputs.push_back(semantic);
                append(declarations,std::vector<uint32_t>{0x0400005fu,0x002190f2u,3,found->reg});
            }
        }
        for (const auto& s:helper_outputs) {
            auto mapped=s;mapped.reg=next++;output_map[s.reg]=mapped.reg;outputs.push_back(mapped);
            if (s.name=="RSFPREV") previous_register=mapped.reg;
            if (s.name=="RSFCUR") current_register=mapped.reg;
        }
        bool temps=false;
        for (auto i:original) {
            if (i.op==104) {i.words[1]=total;temps=true;append(declarations,i.words);continue;}
            if (i.declaration || (i.op>=147 && i.op<=150) || i.op==161 || i.op==162) {append(declarations,i.words);continue;}
            if (i.op==62) continue;
            body.push_back(std::move(i));
        }
        if (!temps) append(declarations,std::vector<uint32_t>{0x02000068u,total});
        for (auto i:added) {
            if (i.op==104 || i.op==106 || i.op==62 || (i.op>=95 && i.op<=97)) continue;
            if (i.op>=101 && i.op<=103) {
                if (i.words.size()<3) return rsf_ac7_velocity_shader_result::refused;
                i.words[2]=output_map.at(i.words[2]);append(declarations,i.words);continue;
            }
            if (i.declaration || i.op==161) {append(declarations,i.words);continue;}
            if (!i.rewrite([&](const Operand& o, OperandEdit& edit) {
                if (o.type==0) return rebase_temp(o,edit,helper_base);
                if (o.type==2) edit.remap(o,output_map.at(o.reg()));
                else if (o.type==1) {
                    const auto m=mapping.at(o.reg());
                    if (m.kind==28) edit={1,28,0,{}};
                    else if (m.kind==25) edit={1,25,2,{m.point,m.reg}};
                    else edit.to_temp(m.reg);
                }
                return true;
            })) return rsf_ac7_velocity_shader_result::refused;
            append(injection,i.words);
        }
        std::vector<uint32_t> code{0x40050,0};append(code,declarations);
        // Run the native domain body at its actual sample, then at the three patch corners.
        // Only private temporaries receive the corner evaluations; native material varyings survive.
        for (uint32_t evaluation=0;evaluation<4;++evaluation) {
            if (evaluation) {
                append(code,std::vector<uint32_t>{0x08000036u,0x001000f2u,domain,0x00004002u,
                    evaluation==1 ? 0x3f800000u : 0u,evaluation==2 ? 0x3f800000u : 0u,evaluation==3 ? 0x3f800000u : 0u,0});
            }
            for (auto i:body) {
                if (i.op==53) {append(code,i.words);continue;}
                if (!i.rewrite([&](const Operand& o, OperandEdit& edit) {
                    if (o.type==0 && evaluation) return rebase_temp(o,edit,base);
                    if (o.type==2) {
                        if (!o.direct(1) || o.reg()>=32) return false;
                        if (evaluation || o.reg()==position)
                            edit.to_temp(evaluation ? scratch+o.reg() : current);
                    }
                    if (o.type==28 && evaluation) edit.to_temp(domain,0);
                    return true;
                },true)) return rsf_ac7_velocity_shader_result::refused;
                append(code,i.words);
            }
            if (evaluation) append(code,std::vector<uint32_t>{0x05000036u,0x001000f2u,corner_base+evaluation-1,0x00100e46u,scratch+position});
        }
        append(code,std::vector<uint32_t>{0x05000036u,0x001020f2u,position,0x00100e46u,current});append(code,injection);append(code,std::vector<uint32_t>{0x0100003eu});
        write_instructions(*shader,code);
        write_signature(*input,inputs);write_signature(*out,outputs);native.write(output);info.outputs=std::move(outputs);info.particle=hull.particle;
        return rsf_ac7_velocity_shader_result::patched;
    } catch (...) {output.clear();info={};return rsf_ac7_velocity_shader_result::refused;}
}

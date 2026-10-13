// SPDX-License-Identifier: GPL-3.0-only
#include "velocity_shader.h"
#include "contact_shadow.h"
#include <rescaleframe/shader_compile.h>
#include <algorithm>
#include <array>
#include <cstring>
#include <string>
#include <map>
#include <sstream>
#include <bit>

namespace {
uint32_t word(const uint8_t* bytes, size_t offset)
{ uint32_t value = 0; std::memcpy(&value, bytes + offset, 4); return value; }
void put(std::vector<uint8_t>& bytes, uint32_t value)
{
    const size_t offset = bytes.size(); bytes.resize(offset + 4);
    std::memcpy(bytes.data() + offset, &value, 4);
}
struct Chunk { std::array<uint8_t,4> tag{}; std::vector<uint8_t> bytes; };
struct Container {
    std::vector<Chunk> chunks;
    bool read(const void* pointer, size_t size) {
        const auto* bytes = static_cast<const uint8_t*>(pointer);
        if (!bytes || size < 32 || size > 8 * 1024 * 1024 || std::memcmp(bytes,"DXBC",4)) return false;
        const uint32_t total = word(bytes,24), count = word(bytes,28);
        if (total > size || total < 32 || count > 32 || size_t(count) * 4 > total - 32) return false;
        for (uint32_t i=0; i<count; ++i) {
            const uint32_t offset = word(bytes,32 + size_t(i)*4);
            if (offset < 32 + count*4 || offset > total || total-offset < 8) return false;
            const uint32_t length = word(bytes,offset+4);
            if (length > total-offset-8) return false;
            Chunk chunk; std::memcpy(chunk.tag.data(),bytes+offset,4);
            chunk.bytes.assign(bytes+offset+8,bytes+offset+8+length);
            chunks.push_back(std::move(chunk));
        }
        return true;
    }
    Chunk* find(const char* tag) {
        for (auto& chunk:chunks) if (!std::memcmp(chunk.tag.data(),tag,4)) return &chunk;
        return nullptr;
    }
    void write(std::vector<uint8_t>& output) {
        output.assign(32+chunks.size()*4,0); std::memcpy(output.data(),"DXBC",4);
        uint32_t value=1; std::memcpy(output.data()+20,&value,4);
        value=uint32_t(chunks.size()); std::memcpy(output.data()+28,&value,4);
        for (size_t i=0;i<chunks.size();++i) {
            value=uint32_t(output.size()); std::memcpy(output.data()+32+i*4,&value,4);
            output.insert(output.end(),chunks[i].tag.begin(),chunks[i].tag.end());
            put(output,uint32_t(chunks[i].bytes.size()));
            output.insert(output.end(),chunks[i].bytes.begin(),chunks[i].bytes.end());
        }
        value=uint32_t(output.size()); std::memcpy(output.data()+24,&value,4);
        rsf_ac7_dxbc_rehash(output.data(),output.size());
    }
};
using Semantic=rsf_ac7_velocity_semantic;
bool read_signature(const Chunk& chunk, std::vector<Semantic>& result)
{
    const auto& bytes=chunk.bytes;
    if (bytes.size()<8) return false;
    const auto count=word(bytes.data(),0);
    const bool streams=!std::memcmp(chunk.tag.data(),"OSG5",4);
    const uint32_t stride=streams ? 28u : 24u;
    if (count>64 || size_t(count)*stride>bytes.size()-8) return false;
    for (uint32_t i=0;i<count;++i) {
        const size_t record=8+size_t(i)*stride;
        if (streams && word(bytes.data(),record)!=0) return false;
        const size_t offset=record+(streams ? 4u : 0u); const auto name=word(bytes.data(),offset);
        if (name>=bytes.size()) return false;
        const auto end=std::find(bytes.begin()+name,bytes.end(),uint8_t(0));
        if (end==bytes.end()) return false;
        Semantic semantic;
        semantic.name.assign(reinterpret_cast<const char*>(bytes.data()+name),size_t(end-(bytes.begin()+name)));
        semantic.index=word(bytes.data(),offset+4); semantic.system=word(bytes.data(),offset+8);
        semantic.type=word(bytes.data(),offset+12); semantic.reg=word(bytes.data(),offset+16);
        semantic.mask=bytes[offset+20]; semantic.used=bytes[offset+21];
        result.push_back(std::move(semantic));
    }
    return true;
}
void write_signature(Chunk& chunk, const std::vector<Semantic>& semantics)
{
    const bool streams=!std::memcmp(chunk.tag.data(),"OSG5",4);
    const uint32_t stride=streams ? 28u : 24u;
    auto ordered=semantics;
    std::stable_sort(ordered.begin(),ordered.end(),[](const auto& a,const auto& b) {
        return std::tie(a.reg,a.mask)<std::tie(b.reg,b.mask);
    });
    auto& bytes=chunk.bytes; bytes.assign(8+ordered.size()*stride,0);
    const uint32_t count=uint32_t(ordered.size()), header=8;
    std::memcpy(bytes.data(),&count,4); std::memcpy(bytes.data()+4,&header,4);
    for (size_t i=0;i<ordered.size();++i) {
        const size_t offset=8+i*stride+(streams ? 4u : 0u); const auto& s=ordered[i];
        const uint32_t name=uint32_t(bytes.size());
        const uint32_t fields[]{name,s.index,s.system,s.type,s.reg};
        std::memcpy(bytes.data()+offset,fields,sizeof(fields));
        bytes[offset+20]=s.mask; bytes[offset+21]=s.used;
        bytes.insert(bytes.end(),s.name.begin(),s.name.end()); bytes.push_back(0);
    }
    while (bytes.size()%4) bytes.push_back(0);
}
struct Operand { size_t token=0,index=0,end=0; uint32_t type=0,reg=0; };
bool operand(const std::vector<uint32_t>& code,size_t& cursor,Operand& out,std::vector<Operand>* descendants=nullptr)
{
    if (cursor>=code.size()) return false;
    out.token=cursor; uint32_t token=code[cursor++]; out.type=(token>>12)&255;
    const uint32_t original=token, dimensions=(token>>20)&3;
    while (token&0x80000000u) { if (cursor>=code.size()) return false; token=code[cursor++]; }
    if (out.type==4 || out.type==5) {
        const uint32_t count=(original&3)==1 ? 1u : (original&3)==2 ? 4u : 0u;
        const size_t length=size_t(count)*(out.type==5 ? 2u : 1u);
        if (!length || length>code.size()-cursor) return false;
        cursor+=length;
    } else {
        for (uint32_t i=0;i<dimensions;++i) {
            const uint32_t representation=(original>>(22+i*3))&7;
            if (representation==0 || representation==1 || representation==3 || representation==4) {
                const size_t length=representation==1 || representation==4 ? 2u : 1u;
                if (length>code.size()-cursor) return false;
                if (!i) { out.index=cursor; out.reg=code[cursor]; }
                cursor+=length;
            }
            if (representation>=2 && representation<=4) {
                Operand relative; if (!operand(code,cursor,relative,descendants)) return false;
                if (descendants) descendants->push_back(relative);
            } else if (representation>4) return false;
        }
    }
    out.end=cursor; return true;
}
struct Instruction {
    std::vector<uint32_t> words; uint32_t op=0;
    bool declaration() const { return op>=88 && op<=106; }
    bool operands(std::vector<Operand>& result,bool nested=false) const {
        size_t cursor=1;
        while (words[cursor-1]&0x80000000u) { if (cursor>=words.size()) return false; ++cursor; }
        std::vector<Operand> children;
        while (cursor<words.size()) { Operand value; if (!operand(words,cursor,value,nested ? &children : nullptr)) return false; result.push_back(value); }
        if (nested) {
            result.insert(result.end(),children.begin(),children.end());
            std::sort(result.begin(),result.end(),[](const auto& a,const auto& b){return a.token<b.token;});
        }
        return true;
    }
};
bool instructions(const Chunk& chunk,std::vector<Instruction>& output,uint32_t profile=0x50)
{
    if (chunk.bytes.size()<8 || chunk.bytes.size()%4 || word(chunk.bytes.data(),0)!=profile ||
        word(chunk.bytes.data(),4)!=chunk.bytes.size()/4) return false;
    for (size_t cursor=2;cursor<chunk.bytes.size()/4;) {
        const auto token=word(chunk.bytes.data(),cursor*4); const uint32_t op=token&0x7ff;
        const uint32_t count=op==53 && cursor+1<chunk.bytes.size()/4 ? word(chunk.bytes.data(),(cursor+1)*4) : (token>>24)&127;
        if (!count || count>chunk.bytes.size()/4-cursor) return false;
        Instruction instruction; instruction.op=op; instruction.words.resize(count);
        std::memcpy(instruction.words.data(),chunk.bytes.data()+cursor*4,size_t(count)*4);
        output.push_back(std::move(instruction)); cursor+=count;
    }
    return true;
}
uint32_t temp_count(const std::vector<Instruction>& code)
{ for (const auto& i:code) if (i.op==104 && i.words.size()==2) return i.words[1]; return 0; }
bool has_cb(const std::vector<Instruction>& code,uint32_t slot)
{
    for (const auto& i:code) if (i.op==89 && i.words.size()>2 && i.words[2]==slot) return true;
    return false;
}
constexpr char helper_source[]=R"(
cbuffer Velocity : register(b13) {
    float4 Rect; float4 Jitter; float Cutoff; uint HasHistory; float2 Padding;
};
float4 main(float4 current_clip:RSFCUR0,float4 previous:RSFPREV0,float4 colour:RSFCOLOR0):SV_Target0 {
    clip(colour.w-Cutoff);
    float2 current=current_clip.xy/current_clip.w-Jitter.xy;
    float2 old=previous.xy/previous.w-Jitter.zw;
    return float4((current-old)*0.2495+32767.0/65535.0,0,0);
})";
const std::vector<uint8_t>& helper()
{
    static const auto bytes=[] { std::vector<uint8_t> result;
        rsf::compile_shader(helper_source,"ac7-translucent-velocity","main","ps_5_0",result); return result; }();
    return bytes;
}
}
rsf_ac7_velocity_shader_result rsf_ac7_velocity_pixel_shader(const void* bytes,size_t size,
    uint32_t blend,std::vector<uint8_t>& output,uint32_t previous_register,uint32_t current_register) noexcept
{
    output.clear();
    try {
        if (blend<2 || blend>5) return rsf_ac7_velocity_shader_result::refused;
        Container native,extra;
        if (!native.read(bytes,size) || !extra.read(helper().data(),helper().size())) return rsf_ac7_velocity_shader_result::refused;
        auto* shader=native.find("SHEX"); if (!shader) shader=native.find("SHDR");
        auto* input=native.find("ISGN"); auto* output_signature=native.find("OSGN");
        auto* add_shader=extra.find("SHEX"); if (!add_shader) add_shader=extra.find("SHDR");
        auto* add_input=extra.find("ISGN");
        std::vector<Instruction> original,added; std::vector<Semantic> inputs,outputs,add_inputs;
        if (!shader || !input || !output_signature || !add_shader || !add_input ||
            !instructions(*shader,original) || !instructions(*add_shader,added) ||
            !read_signature(*input,inputs) || !read_signature(*output_signature,outputs) ||
            !read_signature(*add_input,add_inputs) || has_cb(original,13)) return rsf_ac7_velocity_shader_result::refused;
        bool target=false; for (const auto& s:outputs) {
            if (!_stricmp(s.name.c_str(),"SV_Target")) {
                if (s.index || s.reg) return rsf_ac7_velocity_shader_result::refused;
                target=true;
            }
        }
        if (!target) return rsf_ac7_velocity_shader_result::refused;
        uint32_t current_clip=0,previous=0,extra_current=UINT32_MAX,extra_previous=UINT32_MAX,extra_colour=UINT32_MAX;
        for (auto& s:inputs) {
            previous=std::max(previous,s.reg+1);
            if (s.name=="RSFPREV") return rsf_ac7_velocity_shader_result::refused;
        }
        if (previous_register!=UINT32_MAX) {
            if (previous_register<previous) return rsf_ac7_velocity_shader_result::refused;
            previous=previous_register;
        }
        current_clip=current_register==UINT32_MAX ? previous+1 : current_register;
        if (previous>=32 || current_clip>=32 || current_clip==previous) return rsf_ac7_velocity_shader_result::refused;
        for (const auto& s:inputs) if (s.reg==current_clip) return rsf_ac7_velocity_shader_result::refused;
        for (const auto& s:add_inputs) {
            if (s.name=="RSFCUR") extra_current=s.reg;
            if (s.name=="RSFPREV") extra_previous=s.reg;
            if (s.name=="RSFCOLOR") extra_colour=s.reg;
        }
        if (extra_current==UINT32_MAX || extra_previous==UINT32_MAX || extra_colour==UINT32_MAX)
            return rsf_ac7_velocity_shader_result::refused;
        inputs.push_back({"RSFPREV",0,0,3,previous,15,11});
        inputs.push_back({"RSFCUR",0,0,3,current_clip,15,11});
        const uint32_t native_temps=temp_count(original),colour=native_temps,helper_base=colour+1;
        if (native_temps>4096 || temp_count(added)>4096) return rsf_ac7_velocity_shader_result::refused;
        // Standard AC7 additive base-pass shaders finish with Colour*Opacity, alpha=0, return.
        // Read that preserved scalar, never infer opacity from additive radiance or output alpha.
        std::vector<uint32_t> additive_opacity;
        if (blend==3) {
            if (original.size()<3 || original.back().op!=62) return rsf_ac7_velocity_shader_result::refused;
            size_t multiply_index=original.size(),zero_index=original.size();
            for (size_t j=original.size()-1;j>0;--j) {
                const auto& candidate=original[j-1];
                if (candidate.declaration() || candidate.op==53) continue;
                std::vector<Operand> values;
                if (!candidate.operands(values)) return rsf_ac7_velocity_shader_result::refused;
                if (values.empty() || values[0].type!=2 || values[0].reg!=0) continue;
                const uint32_t mask=(candidate.words[values[0].token]>>4)&15;
                if ((mask&7) && multiply_index==original.size()) multiply_index=j-1;
                if ((mask&8) && zero_index==original.size()) zero_index=j-1;
            }
            if (multiply_index==original.size() || zero_index==original.size()) return rsf_ac7_velocity_shader_result::refused;
            const auto& multiply=original[multiply_index]; const auto& zero=original[zero_index];
            std::vector<Operand> operands,zero_operands;
            if (multiply.op!=56 || zero.op!=54 || original.back().op!=62 ||
                !multiply.operands(operands) || operands.size()!=3 || !zero.operands(zero_operands) || zero_operands.size()!=2 ||
                operands[0].type!=2 || operands[0].reg!=0 || ((multiply.words[operands[0].token]>>4)&15)!=7 ||
                zero_operands[0].type!=2 || zero_operands[0].reg!=0 || ((zero.words[zero_operands[0].token]>>4)&15)!=8 ||
                zero_operands[1].type!=4 || zero.words[zero_operands[1].end-1]!=0)
                return rsf_ac7_velocity_shader_result::refused;
            const Operand* scalar=nullptr;
            for (size_t i=1;i<3;++i) {
                const uint32_t swizzle=(multiply.words[operands[i].token]>>4)&255;
                if (operands[i].type==0 && swizzle==(swizzle&3)*85) {
                    if (scalar) return rsf_ac7_velocity_shader_result::refused;
                    scalar=&operands[i];
                }
            }
            if (!scalar) return rsf_ac7_velocity_shader_result::refused;
            for (size_t j=multiply_index+1;j+1<original.size();++j) {
                std::vector<Operand> values; const auto& candidate=original[j];
                if (candidate.declaration() || candidate.op==53) continue;
                if (!candidate.operands(values)) return rsf_ac7_velocity_shader_result::refused;
                if (!values.empty() && values[0].type==0 && values[0].reg==scalar->reg &&
                    ((candidate.words[values[0].token]>>4)&(1u<<((multiply.words[scalar->token]>>4)&3))))
                    return rsf_ac7_velocity_shader_result::refused;
            }
            additive_opacity.assign(multiply.words.begin()+scalar->token,multiply.words.begin()+scalar->end);
        }
        std::vector<uint32_t> declarations,body,injection; bool temps_declared=false;
        auto append=[](std::vector<uint32_t>& into,const std::vector<uint32_t>& words) { into.insert(into.end(),words.begin(),words.end()); };
        for (auto i:original) {
            if (i.op==63 || i.op==4 || i.op==5) return rsf_ac7_velocity_shader_result::refused;
            if (i.op==104) {
                i.words[1]=helper_base+temp_count(added); temps_declared=true; append(declarations,i.words); continue;
            }
            if (i.declaration()) {
                append(declarations,i.words); continue;
            }
            if (i.op==53) { append(body,i.words); continue; }
            std::vector<Operand> operands;
            if (!i.operands(operands)) return rsf_ac7_velocity_shader_result::refused;
            for (const auto& o:operands) if (o.type==2 && o.reg==0) {
                i.words[o.token]&=~0x000ff000u; i.words[o.index]=colour;
            }
            append(body,i.words);
        }
        if (!temps_declared) append(declarations,{0x02000068u,helper_base+temp_count(added)});
        for (auto i:added) {
            if (i.op==104 || i.op==106 || i.op==101 || i.op==62) continue;
            if (i.op>=98 && i.op<=100) {
                if (i.words.size()<3) return rsf_ac7_velocity_shader_result::refused;
                if (i.words[2]==extra_colour) continue;
                if (i.words[2]==extra_current) i.words[2]=current_clip;
                else if (i.words[2]==extra_previous) i.words[2]=previous;
                else return rsf_ac7_velocity_shader_result::refused;
                append(declarations,i.words); continue;
            }
            if (i.declaration()) { append(declarations,i.words); continue; }
            std::vector<Operand> operands;
            if (!i.operands(operands)) return rsf_ac7_velocity_shader_result::refused;
            for (const auto& o:operands) {
                if (o.type==0) i.words[o.index]+=helper_base;
                if (o.type==1) {
                    if (o.reg==extra_colour) { i.words[o.token]&=~0x000ff000u; i.words[o.index]=colour; }
                    else if (o.reg==extra_current) i.words[o.index]=current_clip;
                    else if (o.reg==extra_previous) i.words[o.index]=previous;
                    else return rsf_ac7_velocity_shader_result::refused;
                }
            }
            append(injection,i.words);
        }
        // Inlining at each unconditional return keeps native material control flow and discards.
        std::vector<uint32_t> code{0x50,0}; append(code,declarations);
        size_t cursor=0;
        while (cursor<body.size()) {
            const uint32_t op=body[cursor]&0x7ff;
            const uint32_t length=op==53 ? body[cursor+1] : (body[cursor]>>24)&127;
            if (op==62) {
                if (!additive_opacity.empty()) {
                    const uint32_t length_mov=uint32_t(3+additive_opacity.size());
                    append(code,{(length_mov<<24)|54u,0x00100082u,colour}); append(code,additive_opacity);
                }
                append(code,injection);
            }
            code.insert(code.end(),body.begin()+cursor,body.begin()+cursor+length); cursor+=length;
        }
        code[1]=uint32_t(code.size()); shader->bytes.resize(code.size()*4);
        std::memcpy(shader->bytes.data(),code.data(),shader->bytes.size());
        write_signature(*input,inputs);
        native.write(output);
        return rsf_ac7_velocity_shader_result::patched;
    } catch (...) { output.clear(); return rsf_ac7_velocity_shader_result::refused; }
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
    output.clear(); info={};
    try {
        Container native;
        if (!native.read(bytes,size)) return rsf_ac7_velocity_shader_result::refused;
        auto* shader=native.find("SHEX"); if (!shader) shader=native.find("SHDR");
        auto* input=native.find("ISGN"); auto* out=native.find("OSGN");
        std::vector<Instruction> original;
        std::vector<Semantic> inputs,outputs;
        if (!shader || !input || !out || !instructions(*shader,original,0x10050) ||
            !read_signature(*input,inputs) || !read_signature(*out,outputs)) return rsf_ac7_velocity_shader_result::refused;
        uint32_t next_input=0,next_output=0;
        const Semantic *position=nullptr,*old=nullptr,*corner=nullptr,*beam_old=nullptr,*beam_size=nullptr,*beam_uv=nullptr;
        bool mesh_rows[3]{};
        for (const auto& s:inputs) {
            next_input=std::max(next_input,s.reg+1);
            if (!_stricmp(s.name.c_str(),"ATTRIBUTE")) {
                if (!s.index && s.mask==15) position=&s;
                if (s.index==1 && s.mask==15) old=&s;
                if (s.index==1 && s.mask==7) beam_old=&s;
                if (s.index==2 && s.mask==15 && !(s.used&8)) beam_size=&s;
                if (s.index==3 && s.mask==15) beam_uv=&s;
                if (s.index==4 && s.mask==3) corner=&s;
                if (s.index>=8 && s.index<=10 && s.mask==15) mesh_rows[s.index-8]=true;
            }
        }
        info.particle=mesh_rows[0] && mesh_rows[1] && mesh_rows[2] ? 2u : position && old && corner ? 1u :
            beam && position && !(position->used&8) && beam_old && beam_size && beam_uv ? 3u : 0u;
        bool has_position=false;
        for (const auto& s:outputs) {
            next_output=std::max(next_output,s.reg+1);
            if (s.system==1) { info.position_register=s.reg; has_position=true; }
            if (tessellation && !_stricmp(s.name.c_str(),"VS_To_DS_Position") && s.mask==15 && s.type==3) {
                info.position_register=s.reg;has_position=true;
            }
            if (s.name.starts_with("RSF")) return rsf_ac7_velocity_shader_result::refused;
        }
        if (!has_position || next_output+4>32) return rsf_ac7_velocity_shader_result::refused;
        static constexpr char generic_source[]=R"(
struct O {float4 p:SV_Position;float4 key:RSFKEY0;float4 centre:RSFCENTRE0;float4 old:RSFOLD0;float4 lookup:RSFLOOKUP0;};
O main(uint vertex:SV_VertexID,uint instance:SV_InstanceID) {O o;o.p=float4(0,0,0,1);
o.key=float4(float(vertex),float(instance),0,0);o.lookup=o.key;o.centre=0;o.old=0;return o;})";
        static constexpr char particle_source[]=R"(
struct O {float4 p:SV_Position;float4 key:RSFKEY0;float4 centre:RSFCENTRE0;float4 old:RSFOLD0;float4 lookup:RSFLOOKUP0;};
O main(float4 centre:ATTRIBUTE0,float4 old:ATTRIBUTE1,float2 corner:ATTRIBUTE4) {
O o;o.p=float4(0,0,0,1);o.key=float4(corner,old.w,1);o.centre=float4(centre.xyz,1);
o.old=float4(old.xyz,1);o.lookup=o.key;return o;})";
        static constexpr char mesh_source[]=R"(
Buffer<float4> Previous:register(t127);
struct O {float4 p:SV_Position;float4 key:RSFKEY0;float4 centre:RSFCENTRE0;float4 old:RSFOLD0;float4 lookup:RSFLOOKUP0;};
uint2 pose_hash(float4 a,float4 b,float4 c) {
    uint2 value=uint2(2166136261,2246822519);
    [unroll]for(uint row=0;row<3;row++) {
        uint4 bits=asuint(row==0 ? a : row==1 ? b : c);
        [unroll]for(uint i=0;i<4;i++) {value=(value^bits[i])*uint2(16777619,3266489917);}
    }
    return value;
}
O main(float4 a:ATTRIBUTE8,float4 b:ATTRIBUTE9,float4 c:ATTRIBUTE10,
       uint vertex:SV_VertexID,uint instance:SV_InstanceID) {
    O o;o.p=float4(0,0,0,1);uint2 now=pose_hash(a,b,c);
    o.key=float4(float(vertex),asfloat(now.x),asfloat(now.y),2);o.centre=float4(a.w,b.w,c.w,1);
    uint count;Previous.GetDimensions(count);o.lookup=float4(float(vertex),0,0,3);o.old=o.centre;
    if(instance<count/3) {
        float4 pa=Previous[instance*3],pb=Previous[instance*3+1],pc=Previous[instance*3+2];
        if(any(pa.xyz!=0) && any(pb.xyz!=0) && any(pc.xyz!=0)) {
            uint2 old=pose_hash(pa,pb,pc);o.lookup=float4(float(vertex),asfloat(old.x),asfloat(old.y),2);
            o.old=float4(pa.w,pb.w,pc.w,1);
        }
    }
    return o;
})";
        static constexpr char beam_source[]=R"(
struct O {float4 p:SV_Position;float4 key:RSFKEY0;float4 centre:RSFCENTRE0;float4 old:RSFOLD0;float4 lookup:RSFLOOKUP0;};
O main(float4 p:ATTRIBUTE0,float4 old:ATTRIBUTE1,float4 size:ATTRIBUTE2,float4 uv:ATTRIBUTE3) {
 O o;o.p=float4(0,0,0,1);o.key=0;o.lookup=float4(0,0,0,3);o.centre=0;o.old=0;
 uint3 bits=asuint(float3(p.w,old.w,size.w));
 uint3 words=(bits&0x1fffffff)|((bits>>2)&0x20000000);uint sample=words.z&4095,intervals=(words.z>>12)&4095;
 if(all((bits&0x60000000)==0x20000000) && words.x>>28<=2 && words.z>>24==0 && intervals!=0 && sample<=intervals && (uv.y==0 || uv.y==1)) {
  float u=float(sample)/float(intervals);o.key=float4(p.w,old.w,floor(u*128),uv.y+4);
  o.lookup=o.key;o.centre=float4(u,0,0,1);o.old=o.centre;
 }
 return o;
})";
        static const auto generic=[] {std::vector<uint8_t> code;rsf::compile_shader(generic_source,"ac7-geometry-key","main","vs_5_0",code);return code;}();
        static const auto particle=[] {std::vector<uint8_t> code;rsf::compile_shader(particle_source,"ac7-particle-key","main","vs_5_0",code);return code;}();
        static const auto mesh=[] {std::vector<uint8_t> code;rsf::compile_shader(mesh_source,"ac7-mesh-particle-pose-key","main","vs_5_0",code);return code;}();
        static const auto beam_code=[] {std::vector<uint8_t> code;rsf::compile_shader(beam_source,"ac7-beam-sample-key","main","vs_5_0",code);return code;}();
        const auto& helper_bytes=info.particle==3 ? beam_code : info.particle==2 ? mesh : info.particle ? particle : generic;
        Container extra; if (!extra.read(helper_bytes.data(),helper_bytes.size())) return rsf_ac7_velocity_shader_result::refused;
        auto* helper_shader=extra.find("SHEX"); if (!helper_shader) helper_shader=extra.find("SHDR");
        auto* helper_input=extra.find("ISGN"); auto* helper_output=extra.find("OSGN");
        std::vector<Instruction> added; std::vector<Semantic> add_inputs,add_outputs;
        if (!helper_shader || !helper_input || !helper_output || !instructions(*helper_shader,added,0x10050) ||
            !read_signature(*helper_input,add_inputs) || !read_signature(*helper_output,add_outputs))
            return rsf_ac7_velocity_shader_result::refused;
        std::map<uint32_t,uint32_t> input_map,output_map;
        uint32_t dummy_position=UINT32_MAX;
        for (const auto& s:add_inputs) {
            auto found=std::find_if(inputs.begin(),inputs.end(),[&](const auto& native_input) {
                return !_stricmp(native_input.name.c_str(),s.name.c_str()) && native_input.index==s.index;
            });
            if (found!=inputs.end()) { input_map[s.reg]=found->reg; found->used|=s.used;found->mask|=s.mask; }
            else {
                if (!s.system || next_input>=32) return rsf_ac7_velocity_shader_result::refused;
                auto appended=s; appended.reg=next_input++; input_map[s.reg]=appended.reg; inputs.push_back(std::move(appended));
            }
        }
        for (const auto& s:add_outputs) {
            if (s.system==1) { dummy_position=s.reg; continue; }
            auto appended=s; appended.reg=next_output++; output_map[s.reg]=appended.reg;
            outputs.push_back(std::move(appended));
        }
        const uint32_t base=temp_count(original),total=base+temp_count(added);
        if (total>4096) return rsf_ac7_velocity_shader_result::refused;
        std::vector<uint32_t> declarations,body,injection;
        auto append=[](auto& into,const auto& values) { into.insert(into.end(),values.begin(),values.end()); };
        std::map<uint32_t,size_t> input_declarations;
        bool temps=false;
        for (auto i:original) {
            if (i.op==4 || i.op==5 || i.op==63) return rsf_ac7_velocity_shader_result::refused;
            if (i.op==104) {i.words[1]=total;temps=true;append(declarations,i.words);continue;}
            if (i.declaration()) {
                if (i.op>=95 && i.op<=97 && i.words.size()>2) input_declarations[i.words[2]]=declarations.size()+1;
                append(declarations,i.words);
            } else append(body,i.words);
        }
        for (auto i:added) {
            if (i.op==104 || i.op==106 || i.op==62) continue;
            if (i.op>=95 && i.op<=97) {
                if (i.words.size()<3 || !input_map.contains(i.words[2])) return rsf_ac7_velocity_shader_result::refused;
                const uint32_t reg=input_map[i.words[2]];
                const auto found=input_declarations.find(reg);
                if (found!=input_declarations.end()) declarations[found->second]|=i.words[1]&0xf0;
                else {i.words[2]=reg;append(declarations,i.words);}
                continue;
            }
            if (i.op>=101 && i.op<=103) {
                if (i.words.size()<3) return rsf_ac7_velocity_shader_result::refused;
                if (i.words[2]==dummy_position) continue;
                if (!output_map.contains(i.words[2])) return rsf_ac7_velocity_shader_result::refused;
                i.words[2]=output_map[i.words[2]]; append(declarations,i.words); continue;
            }
            if (i.declaration()) {append(declarations,i.words);continue;}
            std::vector<Operand> operands;
            if (!i.operands(operands)) return rsf_ac7_velocity_shader_result::refused;
            if (!operands.empty() && operands[0].type==2 && operands[0].reg==dummy_position) continue;
            for (const auto& o:operands) {
                if (o.type==0) i.words[o.index]+=base;
                if (o.type==1) {if (!input_map.contains(o.reg)) return rsf_ac7_velocity_shader_result::refused;i.words[o.index]=input_map[o.reg];}
                if (o.type==2) {if (!output_map.contains(o.reg)) return rsf_ac7_velocity_shader_result::refused;i.words[o.index]=output_map[o.reg];}
            }
            append(injection,i.words);
        }
        if (!temps) {declarations.push_back(0x02000068u);declarations.push_back(total);}
        std::vector<uint32_t> code{0x10050,0}; append(code,declarations);
        for (size_t cursor=0;cursor<body.size();) {
            const uint32_t op=body[cursor]&0x7ff;
            const uint32_t length=op==53 ? body[cursor+1] : (body[cursor]>>24)&127;
            if (op==62) append(code,injection);
            code.insert(code.end(),body.begin()+cursor,body.begin()+cursor+length);cursor+=length;
        }
        code[1]=uint32_t(code.size());shader->bytes.resize(code.size()*4);
        std::memcpy(shader->bytes.data(),code.data(),shader->bytes.size());
        write_signature(*input,inputs);write_signature(*out,outputs);native.write(output);info.outputs=std::move(outputs);
        return rsf_ac7_velocity_shader_result::patched;
    } catch (...) {output.clear();info={};return rsf_ac7_velocity_shader_result::refused;}
}
bool rsf_ac7_velocity_geometry_shader(const rsf_ac7_velocity_vertex_info& info,
    std::vector<uint8_t>& bytecode,uint32_t& previous_register,uint32_t& current_register) noexcept
{
    bytecode.clear(); previous_register=current_register=0;
    try {
        std::ostringstream fields;
        std::string position,key,old,lookup;
        uint32_t count=0;
        for (const auto& s:info.outputs) {
            if (!s.mask || s.mask>15 || s.reg>=32) return false;
            const uint32_t components=std::popcount(uint32_t(s.mask));
            const std::string field="f"+std::to_string(count++);
            fields << (s.type==1 ? "uint" : s.type==2 ? "int" : "float");
            if (components>1) fields << components;
            fields << ' ' << field << ':' << s.name << s.index << ';';
            if (s.system==1) position=field;
            if (s.name=="RSFKEY") key=field;
            if (s.name=="RSFOLD") old=field;
            if (s.name=="RSFLOOKUP") lookup=field;
            previous_register=std::max(previous_register,s.reg+1);
        }
        if (position.empty() || key.empty() || old.empty() || lookup.empty() || previous_register+1>=32) return false;
        current_register=previous_register++;
        std::ostringstream source;
        source << "struct V{" << fields.str() << "};struct O{" << fields.str()
            << "float4 current:RSFCUR0;float4 previous:RSFPREV0;};\n"
R"(
ByteAddressBuffer History:register(t0);
cbuffer Parameters:register(b13) {
    float4 Rect;float4 Jitter;float Cutoff;uint HasHistory;uint HistoryMask;uint Padding;
    row_major float4x4 ClipToPrevious;
};
uint key_hash(uint4 key) {
    uint value=2166136261;
    [unroll]for(uint i=0;i<4;i++) {value=(value^key[i])*16777619;}
    value^=value>>16;return max(1,value&0x7fffffff);
}
uint decode_tag(float value) {uint bits=asuint(value);return (bits&0x1fffffff)|((bits>>2)&0x20000000);}
float encode_tag(uint value) {return asfloat(0x20000000|(value&0x1fffffff)|((value&0x20000000)<<2));}
bool endpoint(float4 key,out float4 position,out bool ambiguous) {
    uint4 bits=asuint(key);uint hash=key_hash(bits);bool found=false;position=0;ambiguous=false;
    [loop]for(uint probe=0;probe<32;probe++) {
        uint address=((hash+probe)&HistoryMask)*64;uint marker=History.Load(address);
        if(marker==0)break;if(marker!=hash || any(History.Load4(address+4)!=bits))continue;
        float4 candidate=asfloat(History.Load4(address+20));if(!all(isfinite(candidate)) || candidate.w<=0)continue;
        if(!found) {position=candidate;found=true;}
        else if(any(asuint(position)!=asuint(candidate)))ambiguous=true;
    }
    return found;
}
float4 previous_position(float4 current,float4 key,float3 old) {
    float4 unjittered=current;unjittered.xy-=Jitter.xy*current.w;
    float4 fallback=mul(unjittered,ClipToPrevious);fallback.xy+=Jitter.zw*fallback.w;
    if(HasHistory==0 || key.w==3)return fallback;
    if(key.w==4 || key.w==5) {
        bool left=false,right=false,bad_left=false,bad_right=false;float low=-1,high=2;float4 a=fallback,b=fallback;
        uint first=decode_tag(key.x),second=decode_tag(key.y);
        if(first>>28!=0) {
            float4 start=key;start.y=encode_tag((second&~0x3ffffff)|(first&0x3ffffff));start.z=0;
            left=endpoint(start,a,bad_left);if(left)low=0;
            float4 end=key;end.x=encode_tag((first&~0x3ffffff)|(second&0x3ffffff));end.z=0;
            right=endpoint(end,b,bad_right);if(right)high=1;
            if(old.x==0)return left && !bad_left ? a : fallback;
            if(old.x==1)return right && !bad_right ? b : fallback;
            // A head point has no prior segment. Its own rendered endpoint still has history.
            if((first&0x3ffffff)==(second&0x3ffffff))return left && !bad_left ? a : fallback;
        }
        uint bin=uint(key.z);
        [loop]for(uint distance=0;distance<=128;distance++) {
            if(left && right && (first>>28==0 || (low>=float(int(bin)-int(distance))/128 && high<=float(bin+distance)/128)))break;
            [unroll]for(uint side=0;side<2;side++) {
                int target=int(bin)+(side==0 ? -int(distance) : int(distance));
                if(target<0 || target>128 || (side==0 && left && low>=float(target+1)/128) || (side==1 && right && high<float(target)/128))continue;
                float4 query=key;query.z=float(target);uint4 bits=asuint(query);uint hash=key_hash(bits);
                [loop]for(uint probe=0;probe<32;probe++) {
                    uint address=((hash+probe)&HistoryMask)*64;uint marker=History.Load(address);
                    if(marker==0)break;if(marker!=hash || any(History.Load4(address+4)!=bits))continue;
                    float4 clip=asfloat(History.Load4(address+20));float u=asfloat(History.Load(address+36));
                    if(!all(isfinite(clip)) || clip.w<=0 || !isfinite(u))continue;
                    if(u<=old.x && (!left || u>low)) {left=true;low=u;a=clip;bad_left=false;}
                    else if(left && u==low && any(asuint(a)!=asuint(clip)))bad_left=true;
                    if(u>=old.x && (!right || u<high)) {right=true;high=u;b=clip;bad_right=false;}
                    else if(right && u==high && any(asuint(b)!=asuint(clip)))bad_right=true;
                }
            }
        }
        if(!left || !right || bad_left || bad_right)return fallback;
        return high==low ? a : lerp(a,b,(old.x-low)/(high-low));
    }
    uint4 bits=asuint(key);uint hash=key_hash(bits);
    bool found=false,ambiguous=false;float distance=3.402823e38;float4 selected=fallback;
    [loop]for(uint probe=0;probe<32;probe++) {
        uint address=((hash+probe)&HistoryMask)*64;
        uint marker=History.Load(address);
        if(marker==0)break;
        if(marker!=hash || any(History.Load4(address+4)!=bits))continue;
        float4 candidate=asfloat(History.Load4(address+20));
        if(!all(isfinite(candidate)) || candidate.w<=0)continue;
        float3 centre=asfloat(History.Load3(address+36));
        float d=key.w==1 ? dot(centre-old,centre-old) : 0;
        if(!found || d<distance) {found=true;distance=d;selected=candidate;ambiguous=false;}
        else if(d==distance && any(asuint(candidate)!=asuint(selected)))ambiguous=true;
    }
    return found && !ambiguous ? selected : fallback;
}
[maxvertexcount(3)]void main(triangle V input[3],inout TriangleStream<O> stream) {
    [unroll]for(uint j=0;j<3;j++) {O output;
)";
        for (size_t i=0;i<info.outputs.size();++i) source << "output.f" << i << "=input[j].f" << i << ';';
        source << "output.current=input[j]." << position << ";output.previous=previous_position(input[j]." << position << ",input[j]." << lookup
            << ",input[j]." << old << ".xyz);stream.Append(output);}stream.RestartStrip();}";
        if (!rsf::compile_shader(source.str().c_str(),"ac7-previous-geometry","main","gs_5_0",bytecode)) return false;
        std::vector<Semantic> actual_inputs;
        if (!rsf_ac7_velocity_signature(bytecode.data(),bytecode.size(),false,actual_inputs)) return false;
        for (const auto& s:info.outputs) {
            const auto found=std::find_if(actual_inputs.begin(),actual_inputs.end(),[&](const auto& actual) {
                return !_stricmp(s.name.c_str(),actual.name.c_str()) && s.index==actual.index;
            });
            if (found==actual_inputs.end() || found->reg!=s.reg || found->mask!=s.mask || found->type!=s.type) return false;
        }
        return true;
    } catch (...) {bytecode.clear();return false;}
}

rsf_ac7_velocity_shader_result rsf_ac7_velocity_native_geometry_shader(const void* bytes,size_t size,
    const rsf_ac7_velocity_vertex_info& vertex,std::vector<uint8_t>& output,
    rsf_ac7_velocity_vertex_info& info,uint32_t& maximum_vertices,
    uint32_t& previous_register,uint32_t& current_register) noexcept
{
    output.clear();info={};maximum_vertices=0;
    try {
        Container native,helper;
        if (!native.read(bytes,size)) return rsf_ac7_velocity_shader_result::refused;
        auto* shader=native.find("SHEX");if (!shader) shader=native.find("SHDR");
        auto* input=native.find("ISGN");auto* out=native.find("OSGN");
        if (!out) out=native.find("OSG5");
        std::vector<Instruction> original;std::vector<Semantic> inputs,outputs;
        if (!shader || !input || !out || !instructions(*shader,original,0x20050) ||
            !read_signature(*input,inputs) || !read_signature(*out,outputs) || has_cb(original,13))
            return rsf_ac7_velocity_shader_result::refused;
        uint32_t position=UINT32_MAX,next=0;
        bool triangle=false;
        for (const auto& s:outputs) {
            if (s.system==1 && s.mask==15 && s.type==3) position=s.reg;
            if (s.name.starts_with("RSF")) return rsf_ac7_velocity_shader_result::refused;
            next=std::max(next,s.reg+1);
        }
        for (const auto& i:original) {
            if (i.op==93) triangle=((i.words[0]>>11)&63)==3;
            if (i.op==94 && i.words.size()==2) maximum_vertices=i.words[1];
            if (i.op==206 && (i.words.size()!=2 || i.words[1]!=1)) return rsf_ac7_velocity_shader_result::refused;
            if ((i.op==88 || i.op==161 || i.op==162) && i.words.size()>2 && i.words[2]==127) return rsf_ac7_velocity_shader_result::refused;
            if (i.op==143 && (i.words.size()!=3 || i.words[2]!=0)) return rsf_ac7_velocity_shader_result::refused;
        }
        if (!triangle || position==UINT32_MAX || !maximum_vertices || maximum_vertices>1024 || next+5>32)
            return rsf_ac7_velocity_shader_result::refused;
        static constexpr char source[]=R"(
ByteAddressBuffer History:register(t127);
cbuffer Parameters:register(b13) {
 float4 Rect;float4 Jitter;float Cutoff;uint HasHistory;uint HistoryMask;uint Padding;
 row_major float4x4 ClipToPrevious;
};
struct O {float4 key:RSFKEY0;float4 centre:RSFCENTRE0;float4 old:RSFOLD0;float4 previous:RSFPREV0;float4 current:RSFCUR0;};
uint4 geometry_key(float4 a,float4 b,float4 c,uint ordinal) {
 uint3 hash=uint3(2166136261,2246822519,3266489917);
 [unroll]for(uint v=0;v<3;v++) {
  uint4 bits=asuint(v==0 ? a : v==1 ? b : c);
  [unroll]for(uint j=0;j<4;j++)hash=(hash^bits[j])*uint3(16777619,3266489917,668265263);
 }
 return uint4(hash,ordinal);
}
O main(float4 p:RSFCLIP0,float4 a:RSFA0,float4 b:RSFA1,float4 c:RSFA2,
       float4 la:RSFB0,float4 lb:RSFB1,float4 lc:RSFB2,
       float4 centre:RSFCENTRE0,float4 old:RSFOLD0,uint ordinal:RSFORDINAL0) {
 O o;o.key=asfloat(geometry_key(a,b,c,ordinal));o.centre=centre;o.old=old;o.current=p;
 float4 unjittered=p;unjittered.xy-=Jitter.xy*p.w;
 o.previous=mul(unjittered,ClipToPrevious);o.previous.xy+=Jitter.zw*o.previous.w;
 uint4 lookup=geometry_key(la,lb,lc,ordinal);uint hash=2166136261;
 [unroll]for(uint j=0;j<4;j++)hash=(hash^lookup[j])*16777619;
 hash^=hash>>16;hash=max(1,hash&0x7fffffff);
 bool found=false,ambiguous=false;float distance=3.402823e38;float4 selected=o.previous;
 if(HasHistory!=0 && la.w!=3 && lb.w!=3 && lc.w!=3) {
  [loop]for(uint probe=0;probe<32;probe++) {
   uint address=((hash+probe)&HistoryMask)*64;uint marker=History.Load(address);
   if(marker==0)break;
   if(marker!=hash || any(History.Load4(address+4)!=lookup))continue;
   float4 candidate=asfloat(History.Load4(address+20));
   if(!all(isfinite(candidate)) || candidate.w<=0)continue;
   float3 centre=asfloat(History.Load3(address+36));
   float d=la.w==1 ? dot(centre-old.xyz,centre-old.xyz) : 0;
   if(!found || d<distance) {found=true;distance=d;selected=candidate;ambiguous=false;}
   else if(d==distance && any(asuint(candidate)!=asuint(selected)))ambiguous=true;
  }
 }
 if(found && !ambiguous)o.previous=selected;return o;
})";
        static const auto helper_bytes=[] {std::vector<uint8_t> code;
            rsf::compile_shader(source,"ac7-native-generated-geometry","main","vs_5_0",code);return code;}();
        if (!helper.read(helper_bytes.data(),helper_bytes.size())) return rsf_ac7_velocity_shader_result::refused;
        auto* hs=helper.find("SHEX");if (!hs) hs=helper.find("SHDR");
        auto* hi=helper.find("ISGN");auto* ho=helper.find("OSGN");
        std::vector<Instruction> added;std::vector<Semantic> helper_inputs,helper_outputs;
        if (!hs || !hi || !ho || !instructions(*hs,added,0x10050) ||
            !read_signature(*hi,helper_inputs) || !read_signature(*ho,helper_outputs)) return rsf_ac7_velocity_shader_result::refused;
        const uint32_t base=temp_count(original),ordinal=base+temp_count(added),position_temp=ordinal+1,total=position_temp+1;
        if (total>4096) return rsf_ac7_velocity_shader_result::refused;
        struct Input {uint32_t kind,reg,vertex;};
        std::map<uint32_t,Input> mapping;std::map<uint32_t,uint32_t> output_map;
        std::vector<uint32_t> declarations,body,injection;
        auto append=[](auto& into,const auto& values) {into.insert(into.end(),values.begin(),values.end());};
        for (const auto& s:helper_inputs) {
            if (s.name=="RSFCLIP") {mapping[s.reg]={0,position_temp,0};continue;}
            if (s.name=="RSFORDINAL") {mapping[s.reg]={0,ordinal,0};continue;}
            const char* name=s.name=="RSFA" ? "RSFKEY" : s.name=="RSFB" ? "RSFLOOKUP" : s.name.c_str();
            const auto found=std::find_if(vertex.outputs.begin(),vertex.outputs.end(),[&](const auto& v){return v.name==name;});
            if (found==vertex.outputs.end()) return rsf_ac7_velocity_shader_result::refused;
            mapping[s.reg]={1,found->reg,s.name=="RSFA" || s.name=="RSFB" ? s.index : 0u};
            if (std::none_of(inputs.begin(),inputs.end(),[&](const auto& v){return v.reg==found->reg;})) {
                auto semantic=*found;semantic.used=semantic.mask;inputs.push_back(semantic);
                append(declarations,std::vector<uint32_t>{0x0400005fu,0x002010f2u,3,found->reg});
            }
        }
        next=0;for (const auto& s:outputs) next=std::max(next,s.reg+1);
        for (const auto& s:helper_outputs) {
            auto mapped=s;mapped.reg=next++;output_map[s.reg]=mapped.reg;outputs.push_back(mapped);
            if (s.name=="RSFPREV") previous_register=mapped.reg;
            if (s.name=="RSFCUR") current_register=mapped.reg;
        }
        bool temps=false;
        for (auto i:original) {
            if (i.op==104) {i.words[1]=total;temps=true;append(declarations,i.words);}
            else if (i.declaration() || i.op==143 || i.op==206) append(declarations,i.words);
            else {
                if (i.op!=53 && i.op!=19 && i.op!=20 && i.op!=117 && i.op!=118 && i.op!=119 && i.op!=9 && i.op!=62) {
                    std::vector<Operand> operands;
                    if (!i.operands(operands)) return rsf_ac7_velocity_shader_result::refused;
                    for (const auto& o:operands) if (o.type==2 && o.reg==position) {
                        i.words[o.token]&=~0x000ff000u;i.words[o.index]=position_temp;
                    }
                }
                append(body,i.words);
            }
        }
        if (!temps) append(declarations,std::vector<uint32_t>{0x02000068u,total});
        for (auto i:added) {
            if (i.op==104 || i.op==106 || i.op==62 || (i.op>=95 && i.op<=97)) continue;
            if (i.op>=101 && i.op<=103) {i.words[2]=output_map.at(i.words[2]);append(declarations,i.words);continue;}
            if (i.declaration() || i.op==161) {append(declarations,i.words);continue;}
            std::vector<Operand> operands;if (!i.operands(operands)) return rsf_ac7_velocity_shader_result::refused;
            // Build backwards because GS inputs add one array index to the helper operand.
            for (auto it=operands.rbegin();it!=operands.rend();++it) {
                const auto& o=*it;
                if (o.type==0) i.words[o.index]+=base;
                else if (o.type==2) i.words[o.index]=output_map.at(o.reg);
                else if (o.type==1) {
                    const auto m=mapping.at(o.reg);
                    i.words[o.token]=(i.words[o.token]&~0x003ff000u)|(m.kind<<12)|(m.kind==1 ? 2u : 1u)<<20;
                    i.words[o.index]=m.reg;
                    if (m.kind==1) i.words.insert(i.words.begin()+o.index,m.vertex);
                }
            }
            i.words[0]=(i.words[0]&~0x7f000000u)|(uint32_t(i.words.size())<<24);append(injection,i.words);
        }
        std::vector<uint32_t> code{0x20050,0};append(code,declarations);
        append(code,std::vector<uint32_t>{0x05000036u,0x00100012u,ordinal,0x00004001u,0});
        for (size_t cursor=0;cursor<body.size();) {
            const uint32_t op=body[cursor]&0x7ff,length=op==53 ? body[cursor+1] : (body[cursor]>>24)&127;
            if (op==19 || op==20 || op==117 || op==119) {
                append(code,std::vector<uint32_t>{0x05000036u,0x001020f2u,position,0x00100e46u,position_temp});
                append(code,injection);
                append(code,std::vector<uint32_t>{0x0700001eu,0x00100012u,ordinal,0x0010000au,ordinal,0x00004001u,1});
            }
            code.insert(code.end(),body.begin()+cursor,body.begin()+cursor+length);cursor+=length;
        }
        code[1]=uint32_t(code.size());shader->bytes.resize(code.size()*4);std::memcpy(shader->bytes.data(),code.data(),shader->bytes.size());
        write_signature(*input,inputs);write_signature(*out,outputs);native.write(output);info.outputs=std::move(outputs);info.particle=vertex.particle;
        return rsf_ac7_velocity_shader_result::patched;
    } catch (...) {output.clear();info={};return rsf_ac7_velocity_shader_result::refused;}
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
        auto save=[&](std::vector<uint32_t>& code,std::vector<uint8_t>& destination) {
            code[1]=uint32_t(code.size());shader->bytes.resize(code.size()*4);std::memcpy(shader->bytes.data(),code.data(),shader->bytes.size());native.write(destination);
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
        std::vector<uint32_t> declarations,body,injection;
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
            if (i.declaration() || (i.op>=147 && i.op<=150) || i.op==161 || i.op==162) {append(declarations,i.words);continue;}
            if (i.op==62) continue;
            append(body,i.words);
        }
        if (!temps) append(declarations,std::vector<uint32_t>{0x02000068u,total});
        for (auto i:added) {
            if (i.op==104 || i.op==106 || i.op==62 || (i.op>=95 && i.op<=97)) continue;
            if (i.op>=101 && i.op<=103) {i.words[2]=output_map.at(i.words[2]);append(declarations,i.words);continue;}
            if (i.declaration() || i.op==161) {append(declarations,i.words);continue;}
            std::vector<Operand> operands;if (!i.operands(operands)) return rsf_ac7_velocity_shader_result::refused;
            for (auto it=operands.rbegin();it!=operands.rend();++it) {
                const auto& o=*it;
                if (o.type==0) i.words[o.index]+=helper_base;
                else if (o.type==2) i.words[o.index]=output_map.at(o.reg);
                else if (o.type==1) {
                    const auto m=mapping.at(o.reg);
                    i.words[o.token]=(i.words[o.token]&~0x003ff000u)|(m.kind<<12)|((m.kind==25 ? 2u : m.kind==28 ? 0u : 1u)<<20);
                    if (m.kind==28) i.words.erase(i.words.begin()+o.index);
                    else {i.words[o.index]=m.reg;if (m.kind==25) i.words.insert(i.words.begin()+o.index,m.point);}
                }
            }
            i.words[0]=(i.words[0]&~0x7f000000u)|(uint32_t(i.words.size())<<24);append(injection,i.words);
        }
        std::vector<uint32_t> code{0x40050,0};append(code,declarations);
        // Run the native domain body at its actual sample, then at the three patch corners.
        // Only private temporaries receive the corner evaluations; native material varyings survive.
        for (uint32_t evaluation=0;evaluation<4;++evaluation) {
            if (evaluation) {
                append(code,std::vector<uint32_t>{0x08000036u,0x001000f2u,domain,0x00004002u,
                    evaluation==1 ? 0x3f800000u : 0u,evaluation==2 ? 0x3f800000u : 0u,evaluation==3 ? 0x3f800000u : 0u,0});
            }
            for (size_t cursor=0;cursor<body.size();) {
                Instruction i;i.op=body[cursor]&0x7ff;const uint32_t length=i.op==53 ? body[cursor+1] : (body[cursor]>>24)&127;
                i.words.assign(body.begin()+cursor,body.begin()+cursor+length);cursor+=length;
                if (i.op==53) {append(code,i.words);continue;}
                std::vector<Operand> operands;if (!i.operands(operands,true)) return rsf_ac7_velocity_shader_result::refused;
                for (auto it=operands.rbegin();it!=operands.rend();++it) {
                    const auto& o=*it;
                    if (o.type==0 && evaluation) i.words[o.index]+=base;
                    if (o.type==2 && (evaluation || o.reg==position)) {
                        i.words[o.token]&=~0x000ff000u;i.words[o.index]=evaluation ? scratch+o.reg : current;
                    }
                    if (o.type==28 && evaluation) {
                        i.words[o.token]=(i.words[o.token]&~0x003ff000u)|0x00100000u;
                        i.words.insert(i.words.begin()+o.token+1,domain);
                    }
                }
                i.words[0]=(i.words[0]&~0x7f000000u)|(uint32_t(i.words.size())<<24);append(code,i.words);
            }
            if (evaluation) append(code,std::vector<uint32_t>{0x05000036u,0x001000f2u,corner_base+evaluation-1,0x00100e46u,scratch+position});
        }
        append(code,std::vector<uint32_t>{0x05000036u,0x001020f2u,position,0x00100e46u,current});append(code,injection);append(code,std::vector<uint32_t>{0x0100003eu});
        code[1]=uint32_t(code.size());shader->bytes.resize(code.size()*4);std::memcpy(shader->bytes.data(),code.data(),shader->bytes.size());
        write_signature(*input,inputs);write_signature(*out,outputs);native.write(output);info.outputs=std::move(outputs);info.particle=hull.particle;
        return rsf_ac7_velocity_shader_result::patched;
    } catch (...) {output.clear();info={};return rsf_ac7_velocity_shader_result::refused;}
}

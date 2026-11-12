// SPDX-License-Identifier: GPL-3.0-only
#include "dxbc.h"
#include "patch.h"
#include <array>
#include <bit>
#include <cmath>
#include <cstring>
#include <vector>
namespace rsf::patch {
namespace {
uint32_t word(std::string_view s,size_t at) {
    check(at<=s.size()&&s.size()-at>=4,RSF_PATCH_FORMAT,"DXBC word");uint32_t n{};std::memcpy(&n,s.data()+at,4);return n;
}
// MD5 compression, with DXBC's container-specific final-block layout.
std::array<uint32_t,4> checksum(std::string_view s) {
    check(s.size()%4==0&&s.size()<=UINT32_MAX,RSF_PATCH_FORMAT,"DXBC alignment");
    std::vector<uint32_t> data(s.size()/4);std::memcpy(data.data(),s.data(),s.size());
    const size_t full=data.size()/16*16,tail=data.size()-full;
    const auto length=static_cast<uint32_t>(s.size());
    if(tail<14){data.insert(data.begin()+static_cast<ptrdiff_t>(full),length<<3);data.push_back(0x80);data.resize(full+15,0);data.push_back(length*2+1);}
    else {data.push_back(0x80);data.resize(full+16,0);data.push_back(length<<3);data.resize(full+31,0);data.push_back(length*2+1);}
    std::array<uint32_t,4> h{0x67452301,0xefcdab89,0x98badcfe,0x10325476};
    constexpr unsigned shifts[4][4]={{7,12,17,22},{5,9,14,20},{4,11,16,23},{6,10,15,21}};
    for(size_t block=0;block<data.size();block+=16){
        auto a=h[0],b=h[1],c=h[2],d=h[3];
        for(unsigned i=0;i<64;++i){uint32_t f{},g{};
            if(i<16){f=(b&c)|(~b&d);g=i;}
            else if(i<32){f=(d&b)|(~d&c);g=(5*i+1)%16;}
            else if(i<48){f=b^c^d;g=(3*i+5)%16;}
            else {f=c^(b|~d);g=(7*i)%16;}
            const auto k=static_cast<uint32_t>(std::floor(std::abs(std::sin(double(i+1)))*4294967296.0));
            const auto next=b+std::rotl(a+f+k+data[block+g],static_cast<int>(shifts[i/16][i%4]));
            a=d;d=c;c=b;b=next;
        }
        h[0]+=a;h[1]+=b;h[2]+=c;h[3]+=d;
    }
    return h;
}
}
std::string rewrite_dxbc(std::string_view input,std::string_view hash,std::span<const uint32_t> pattern,
    std::span<const uint32_t> replacement) {
    check(sha256(input)==hash,RSF_PATCH_HASH,"DXBC hash");
    check(input.size()>=32&&input.substr(0,4)=="DXBC"&&word(input,24)==input.size(),RSF_PATCH_FORMAT,"DXBC header");
    check(!pattern.empty()&&pattern.size()==replacement.size()&&(pattern[0]>>24&127)==pattern.size()&&
        (replacement[0]>>24&127)==replacement.size(),RSF_PATCH_ARGUMENT,"whole same-size instruction required");
    const auto old=checksum(input.substr(20));
    check(std::memcmp(old.data(),input.data()+4,16)==0,RSF_PATCH_FORMAT,"DXBC checksum");
    const auto chunks=word(input,28);check(chunks<=(input.size()-32)/4,RSF_PATCH_FORMAT,"DXBC chunks");
    size_t match=0,count=0;
    for(uint32_t i=0;i<chunks;++i){
        const size_t at=word(input,32+i*4);const size_t size=word(input,at+4);
        check(at>=32+chunks*4&&at+8<=input.size()&&size<=input.size()-at-8,RSF_PATCH_FORMAT,"DXBC chunk size");
        const auto kind=input.substr(at,4);
        if(kind!="SHDR"&&kind!="SHEX")continue;
        check(size>=8&&size%4==0&&word(input,at+12)==size/4,RSF_PATCH_FORMAT,"token program length");
        for(size_t pos=at+16;pos<at+8+size;){
            const auto opcode=word(input,pos);size_t n=(opcode>>24)&127;
            if((opcode&0x7ff)==53)n=word(input,pos+4);
            check(n&&n<=(at+8+size-pos)/4,RSF_PATCH_FORMAT,"instruction length");
            if(n==pattern.size()&&std::memcmp(input.data()+pos,pattern.data(),n*4)==0){match=pos;++count;}
            pos+=n*4;
        }
    }
    check(count==1,RSF_PATCH_ANCHOR,"missing or ambiguous DXBC instruction");
    std::string output(input);std::memcpy(output.data()+match,replacement.data(),replacement.size_bytes());
    const auto digest=checksum(std::string_view(output).substr(20));std::memcpy(output.data()+4,digest.data(),16);return output;
}
}

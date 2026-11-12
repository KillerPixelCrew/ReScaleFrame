// SPDX-License-Identifier: GPL-3.0-only
#include "patch.h"
#include <windows.h>
#include <bcrypt.h>
#include <algorithm>
#include <array>
#include <cctype>
#include <cstring>
#include <memory>
#include <set>

namespace rsf::patch {
void check(bool ok,int32_t code,const char* reason) { if(!ok) throw Error{code,reason}; }
std::string_view bytes(rsf_patch_bytes b) {
    check(b.data || !b.size,RSF_PATCH_ARGUMENT,"null bytes");
    return b.size ? std::string_view(static_cast<const char*>(b.data),b.size) : std::string_view{};
}
std::string sha256(std::string_view data) {
    check(data.size() <= ULONG_MAX,RSF_PATCH_ARGUMENT,"hash input too large");
    BCRYPT_ALG_HANDLE algorithm{};
    check(BCryptOpenAlgorithmProvider(&algorithm,BCRYPT_SHA256_ALGORITHM,nullptr,0)>=0,RSF_PATCH_INTERNAL,"SHA provider");
    std::array<unsigned char,32> digest{};
    const auto status=BCryptHash(algorithm,nullptr,0,reinterpret_cast<PUCHAR>(const_cast<char*>(data.data())),
        static_cast<ULONG>(data.size()),digest.data(),static_cast<ULONG>(digest.size()));
    BCryptCloseAlgorithmProvider(algorithm,0);
    check(status>=0,RSF_PATCH_INTERNAL,"SHA failure");
    std::string out;
    for(auto n:digest) {out += "0123456789abcdef"[n>>4];out += "0123456789abcdef"[n&15];}
    return out;
}
namespace {
uint64_t integer(std::string_view data,size_t at,size_t n) {
    check(at<=data.size() && n<=data.size()-at,RSF_PATCH_FORMAT,"truncated integer");
    uint64_t value=0;
    for(size_t i=0;i<n;++i)value |= uint64_t(static_cast<unsigned char>(data[at+i]))<<(8*i);
    return value;
}
std::string_view slice(std::string_view data,uint64_t at,uint64_t n) {
    check(at<=data.size() && n<=data.size()-at,RSF_PATCH_FORMAT,"section outside file");
    return data.substr(static_cast<size_t>(at),static_cast<size_t>(n));
}
std::string string_value(yyjson_val* value) {
    check(yyjson_is_str(value),RSF_PATCH_FORMAT,"expected string");
    std::string s(yyjson_get_str(value),yyjson_get_len(value));
    check(s.find('\0')==s.npos,RSF_PATCH_FORMAT,"embedded NUL");
    return s;
}
void unique_keys(yyjson_val* value, size_t depth=0) {
    check(depth<128,RSF_PATCH_FORMAT,"JSON nesting");
    if(yyjson_is_obj(value)) {
        std::set<std::string> keys;size_t i,n;yyjson_val *k,*v;
        yyjson_obj_foreach(value,i,n,k,v) {
            check(keys.insert(string_value(k)).second,RSF_PATCH_FORMAT,"duplicate JSON key");unique_keys(v,depth+1);
        }
    } else if(yyjson_is_arr(value)) {
        size_t i,n;yyjson_val* v;yyjson_arr_foreach(value,i,n,v)unique_keys(v,depth+1);
    }
}
yyjson_doc* json(std::string_view data) {
    auto* doc=yyjson_read(data.data(),data.size(),0);
    check(doc!=nullptr,RSF_PATCH_FORMAT,"invalid JSON");
    try {unique_keys(yyjson_doc_get_root(doc));}catch(...){yyjson_doc_free(doc);throw;}
    return doc;
}
size_t unique_at(const std::string& source,const std::string& anchor) {
    check(!anchor.empty(),RSF_PATCH_FORMAT,"empty anchor");
    const auto at=source.find(anchor);
    check(at!=source.npos && source.find(anchor,at+1)==source.npos,RSF_PATCH_ANCHOR,"missing or ambiguous anchor");
    return at;
}
std::string remap(std::string_view source,yyjson_val* table) {
    check(yyjson_is_obj(table),RSF_PATCH_FORMAT,"remap table");
    std::string out;
    for(size_t i=0;i<source.size();) {
        const auto c=static_cast<unsigned char>(source[i]);
        if(std::isalpha(c) || c=='_') {
            const size_t begin=i++;
            while(i<source.size() && (std::isalnum(static_cast<unsigned char>(source[i])) || source[i]=='_'))++i;
            const std::string word(source.substr(begin,i-begin));
            auto* replacement=yyjson_obj_get(table,word.c_str());
            out += replacement ? string_value(replacement) : word;
        } else out+=source[i++];
    }
    return out;
}
}
std::string text(yyjson_val* value,const char* key) {return string_value(yyjson_obj_get(value,key));}
Pack::~Pack(){if(rules)yyjson_doc_free(rules);}
void Pack::load(std::string_view data) {
    check(!rules,RSF_PATCH_ARGUMENT,"pack already loaded");
    check(slice(data,0,8)=="RSFPATCH" && integer(data,8,4)==1,RSF_PATCH_FORMAT,"pack version");
    const uint64_t count=integer(data,12,4),table=integer(data,16,8);
    check(count<=data.size()/24 && table>=24,RSF_PATCH_FORMAT,"section table");
    (void)slice(data,table,count*24);
    std::vector<std::pair<uint64_t,uint64_t>> ranges{{0,24},{table,table+count*24}};
    std::map<uint32_t,std::string_view> sections;
    for(uint64_t i=0;i<count;++i) {
        const auto at=static_cast<size_t>(table+i*24);
        const auto type=static_cast<uint32_t>(integer(data,at,4));
        check(integer(data,at+4,4)==0,RSF_PATCH_FORMAT,"reserved section field");
        const auto off=integer(data,at+8,8),size=integer(data,at+16,8);
        auto part=slice(data,off,size);ranges.emplace_back(off,off+size);
        check(sections.emplace(type,part).second,RSF_PATCH_FORMAT,"duplicate section");
    }
    std::sort(ranges.begin(),ranges.end());
    for(size_t i=1;i<ranges.size();++i)check(ranges[i].first>=ranges[i-1].second,RSF_PATCH_FORMAT,"overlapping sections");
    check(sections.contains(1)&&sections.contains(2)&&sections.contains(3),RSF_PATCH_FORMAT,"missing section");
    auto* meta=json(sections.at(1));
    try {id=text(yyjson_doc_get_root(meta),"id");check(!id.empty(),RSF_PATCH_FORMAT,"empty pack id");}
    catch(...){yyjson_doc_free(meta);throw;}
    yyjson_doc_free(meta);
    rules=json(sections.at(2));
    check(yyjson_get_uint(yyjson_obj_get(yyjson_doc_get_root(rules),"schema"))==1,RSF_PATCH_FORMAT,"rule schema");
    auto files=sections.at(3);size_t at=4;
    const auto n=integer(files,0,4);
    check(n<=files.size()/12,RSF_PATCH_FORMAT,"payload table");
    for(uint64_t i=0;i<n;++i) {
        const auto len=integer(files,at,4),size=integer(files,at+4,8);at+=12;
        const std::string name(slice(files,at,len));at+=static_cast<size_t>(len);
        check(!name.empty() && name.find('\0')==name.npos,RSF_PATCH_FORMAT,"payload name");
        const std::string content(slice(files,at,size));at+=static_cast<size_t>(size);
        check(payload.emplace(name,content).second,RSF_PATCH_FORMAT,"duplicate payload");
    }
    check(at==files.size(),RSF_PATCH_FORMAT,"trailing payload");
    hash=sha256(data);
}
Files Pack::transform(const Files& originals) const {
    Files out=originals;
    auto* root=yyjson_doc_get_root(rules);
    auto* sources=yyjson_obj_get(root,"sources");
    check(yyjson_is_obj(sources),RSF_PATCH_FORMAT,"source hashes");
    size_t i,n;yyjson_val *k,*v;
    yyjson_obj_foreach(sources,i,n,k,v) {
        const auto name=string_value(k);auto found=originals.find(name);
        check(found!=originals.end(),RSF_PATCH_SOURCE,"source missing");
        check(sha256(found->second)==string_value(v),RSF_PATCH_HASH,"source hash mismatch");
    }
    auto* edits=yyjson_obj_get(root,"files");check(yyjson_is_obj(edits),RSF_PATCH_FORMAT,"file rules");
    yyjson_obj_foreach(edits,i,n,k,v) {
        const auto name=string_value(k),op=text(v,"op");std::string source;
        const auto input=op=="extract" ? text(v,"source") : name;
        const auto found=originals.find(input);check(found!=originals.end(),RSF_PATCH_SOURCE,"rule input missing");
        source=found->second;
        if(op=="wrap")out[name]=text(v,"prefix")+source+text(v,"suffix");
        else if(op=="inserts") {
            auto* list=yyjson_obj_get(v,"edits");check(yyjson_is_arr(list),RSF_PATCH_FORMAT,"insertion list");
            size_t a,b;yyjson_val* edit;size_t last=source.size()+1;
            yyjson_arr_foreach(list,a,b,edit) {
                const auto at=yyjson_get_uint(yyjson_obj_get(edit,"at"));
                const auto anchor_at=yyjson_get_uint(yyjson_obj_get(edit,"anchor_at"));
                const auto anchor=text(edit,"anchor");
                check(at<last && at<=source.size() && anchor_at<=source.size() &&
                    found->second.compare(static_cast<size_t>(anchor_at),anchor.size(),anchor)==0,RSF_PATCH_ANCHOR,"insertion anchor");
                source.insert(static_cast<size_t>(at),text(edit,"text"));last=static_cast<size_t>(at);
            }
            out[name]=std::move(source);
        } else if(op=="extract") {
            const auto start=unique_at(source,text(v,"start")),end=source.rfind(text(v,"end"));
            check(end!=source.npos && end>start,RSF_PATCH_ANCHOR,"extract boundary");
            out[name]=text(v,"prefix")+remap(std::string_view(source).substr(start,end-start),yyjson_obj_get(v,"remap"))+text(v,"suffix");
        } else if(op=="clone_function") {
            if(source.starts_with("\xef\xbb\xbf"))source.erase(0,3);
            const auto start=unique_at(source,text(v,"start"));const auto body=source.find('{',start);
            check(body!=source.npos,RSF_PATCH_ANCHOR,"function body");
            size_t end=body+1;int depth=1;
            while(end<source.size() && depth) {depth+=(source[end]=='{')-(source[end]=='}');++end;}
            check(!depth,RSF_PATCH_ANCHOR,"unbalanced function");
            const auto borrowed=source.substr(body,end-body);
            out[name]=text(v,"prefix")+source.substr(0,start)+text(v,"before")+borrowed+
                text(v,"between")+remap(borrowed,yyjson_obj_get(v,"remap"))+text(v,"after")+source.substr(end);
        } else throw Error{RSF_PATCH_FORMAT,"unknown source operation"};
    }
    for(const auto& [name,content]:payload)check(out.emplace(name,content).second,RSF_PATCH_FORMAT,"payload overrides acquired source");
    return out;
}
}

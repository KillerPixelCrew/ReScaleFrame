// SPDX-License-Identifier: GPL-3.0-only
// A parameter object laid out like NVSDK_NGX_Parameter (nvsdk_ngx_params.h): the declaration order
// below is the SDK's, so the compiler arranges the vtable the way the vendor's DLL would.
#pragma once

#include <d3d11.h>
#include <d3d12.h>

#include <cstring>
#include <map>
#include <string>

struct NgxParam {
    virtual void Set(const char* name, unsigned long long v) = 0;
    virtual void Set(const char* name, float v) = 0;
    virtual void Set(const char* name, double v) = 0;
    virtual void Set(const char* name, unsigned int v) = 0;
    virtual void Set(const char* name, int v) = 0;
    virtual void Set(const char* name, ID3D11Resource* v) = 0;
    virtual void Set(const char* name, ID3D12Resource* v) = 0;
    virtual void Set(const char* name, void* v) = 0;

    virtual int Get(const char* name, unsigned long long* v) const = 0;
    virtual int Get(const char* name, float* v) const = 0;
    virtual int Get(const char* name, double* v) const = 0;
    virtual int Get(const char* name, unsigned int* v) const = 0;
    virtual int Get(const char* name, int* v) const = 0;
    virtual int Get(const char* name, ID3D11Resource** v) const = 0;
    virtual int Get(const char* name, ID3D12Resource** v) const = 0;
    virtual int Get(const char* name, void** v) const = 0;

    virtual void Reset() = 0;
};

class FakeNgxParams final : public NgxParam {
public:
    void Set(const char* n, unsigned long long v) override { values_[n] = {Kind::Num, static_cast<double>(v), nullptr}; }
    void Set(const char* n, float v) override { values_[n] = {Kind::Num, v, nullptr}; }
    void Set(const char* n, double v) override { values_[n] = {Kind::Num, v, nullptr}; }
    void Set(const char* n, unsigned int v) override { values_[n] = {Kind::Num, static_cast<double>(v), nullptr}; }
    void Set(const char* n, int v) override { values_[n] = {Kind::Num, static_cast<double>(v), nullptr}; }
    void Set(const char* n, ID3D11Resource* v) override { values_[n] = {Kind::Ptr, 0, v}; }
    void Set(const char* n, ID3D12Resource* v) override { values_[n] = {Kind::Ptr, 0, v}; }
    void Set(const char* n, void* v) override { values_[n] = {Kind::Ptr, 0, v}; }

    int Get(const char* n, unsigned long long* v) const override { return number(n, v); }
    int Get(const char* n, float* v) const override { return number(n, v); }
    int Get(const char* n, double* v) const override { return number(n, v); }
    int Get(const char* n, unsigned int* v) const override { return number(n, v); }
    int Get(const char* n, int* v) const override { return number(n, v); }
    int Get(const char* n, ID3D11Resource** v) const override { return pointer(n, reinterpret_cast<void**>(v)); }
    int Get(const char* n, ID3D12Resource** v) const override { return pointer(n, reinterpret_cast<void**>(v)); }
    int Get(const char* n, void** v) const override { return pointer(n, v); }

    void Reset() override { values_.clear(); }

private:
    enum class Kind { Num, Ptr };
    struct Value {
        Kind kind;
        double number;
        void* ptr;
    };
    template <class T>
    int number(const char* n, T* out) const {
        auto it = values_.find(n);
        if (it == values_.end() || it->second.kind != Kind::Num) return 0xBAD00000;
        *out = static_cast<T>(it->second.number);
        return 1; // NVSDK_NGX_Result_Success
    }
    int pointer(const char* n, void** out) const {
        auto it = values_.find(n);
        if (it == values_.end() || it->second.kind != Kind::Ptr) return 0xBAD00000;
        *out = it->second.ptr;
        return 1;
    }
    std::map<std::string, Value> values_;
};

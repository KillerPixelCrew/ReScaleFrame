// SPDX-License-Identifier: MIT
// Keep the SDK declaration order so native fixture callers independently select the SDK slots.
// The fixture DLL supplies its parameter table and values in Rust.
#pragma once

#include <d3d11.h>
#include <d3d12.h>

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

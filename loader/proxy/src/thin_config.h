#pragma once
#include <windows.h>
#include <string>

struct ThinConfig {
    bool auto_start = false;
    std::wstring runtime_module = L"ReScaleFrame/ReScaleFrame.Runtime.dll";
    std::wstring entry_module;
    std::string entry_point;
};

// Bounded, strict INI decoding; does not load code or consult INI registry mappings.
bool thin_read_config(HANDLE file, ThinConfig& config, std::string& error);

// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include "shader_loader.h"
#include "shader_d3d.h"
#include <rescaleframe/shader_patch.h>
namespace rsf::anomaly::motion {
class RuntimeShaders {
public:
    ~RuntimeShaders();
    bool open(const std::filesystem::path&,const std::filesystem::path&,std::byte*,const MotionProfile&,std::string&);
    Manifest manifest;
    Prepared prepare(const ShaderRequest&,const ManifestEntry&,D3DShaderApi&);
    const rsf_shader_patch_api& api() const noexcept { return api_; }
private:
    rsf_shader_patch_api api_{};
    rsf_patch_session* session_{};
    void* include_table_{};
    std::string source_;
    static int32_t resolve(void*,const char*,rsf_patch_bytes*) noexcept;
};
}

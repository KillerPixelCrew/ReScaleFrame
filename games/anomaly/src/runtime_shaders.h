// SPDX-License-Identifier: MIT
#pragma once
#include "shader_loader.h"
#include "shader_d3d.h"
#include <rescaleframe/shader_patch.h>
namespace rsf::anomaly::motion {
struct RuntimePack;
class RuntimeShaders {
public:
    RuntimeShaders() = default;
    RuntimeShaders(const RuntimeShaders&) = delete;
    RuntimeShaders& operator=(const RuntimeShaders&) = delete;
    ~RuntimeShaders();
    bool open(const std::filesystem::path&,const std::filesystem::path&,std::byte*,const MotionProfile&,std::string&);
    const rsf_anomaly_shader_admitted* manifest() const noexcept;
    Prepared prepare(const ShaderRequest&,const ManifestEntry&,D3DShaderApi&);
    const rsf_shader_patch_api& api() const noexcept;
private:
    RuntimePack* pack_{};
    void* include_table_{};
    std::string source_;
    static int32_t resolve(void*,const char*,rsf_patch_bytes*) noexcept;
};
}

"""CPU regression of the actual common SR admission and backend dispatch prefix."""
from pathlib import Path
import subprocess
root=Path(__file__).resolve().parents[1]
out=root/"build/checks/worker-srvk-xess-range";out.mkdir(parents=True,exist_ok=True)
s=(root/"runtime/orchestrator/src/native_sr_vulkan.cpp").read_text(encoding="utf-8")
a=s.index('bool valid_image(');b=s.index('\n}',a)+2
valid=s[a:b]
a=s.index('extern "C" RSF_RUNTIME_API rsf_backend_result rsf_srvk_evaluate(')
b=s.index('    auto list = static_cast<VkCommandBuffer>(commands);',a)
evaluate=s[a:b]+'    return RSF_BACKEND_OK;\n#else\n    return RSF_BACKEND_ERROR_NOT_COMPILED;\n#endif\n} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }\n'
code=r'''#include <rescaleframe/native_sr_vulkan.h>
#include <vulkan/vulkan.h>
#include <cmath>
#include <cstddef>
#include <cstring>
#include <cstdio>
#include <cstdlib>
#include <initializer_list>
#define CHECK(c) do {if(!(c)){std::fprintf(stderr,"failed: %s\n",#c);std::abort();}}while(0)
struct Capture {template<class... T> bool record(T...) {return false;}};
struct rsf_srvk {uint32_t width=1130,height=636;void *xess=nullptr,*dlss=nullptr;rsf_srvk_setup setup{};Capture capture;};
static unsigned xess_calls,dlss_calls;
static int rsf_input_dump_begin_record(const rsf_frame_record*,uint32_t){return 0;}
static bool rsf_input_dump_wants(const rsf_frame_record*){return false;}
static void rsf_input_dump_result(const rsf_frame_record*,const char*,uint32_t,rsf_backend_result){}
static rsf_backend_result rsf_xessvk_evaluate(void*,void*,const rsf_srvk_frame *f) {
    ++xess_calls; CHECK(f->color.width==f->record->render_width);
    return f->color.width<1130 || f->color.height<636 || f->color.width>1280 || f->color.height>720
        ? RSF_BACKEND_ERROR_FEATURE_FAILED:RSF_BACKEND_OK;
}
static rsf_backend_result rsf_dlssvk_evaluate(void*,void*,const rsf_srvk_frame*) {++dlss_calls;return RSF_BACKEND_OK;}
'''+valid+'\n'+evaluate+r'''
int main(){
    rsf_srvk self; self.xess=&self;self.setup.device=&self;
    self.setup.output_width=1920;self.setup.output_height=1080;
    rsf_frame_record r{};r.struct_size=sizeof r;r.abi_version=RSF_GAME_FRAME_ABI_VERSION;
    r.camera.struct_size=sizeof r.camera;r.camera.abi_version=RSF_GAME_CAMERA_ABI_VERSION;
    r.session_id=r.frame_id=1;r.frame_time_ms=16;r.camera.near_plane=.1f;r.camera.far_plane=1000;r.camera.vertical_fov_radians=1;
    r.output_width=r.camera.output_width=1920;r.output_height=r.camera.output_height=1080;
    rsf_srvk_frame f{};f.struct_size=sizeof f;f.abi_version=RSF_SRVK_ABI_VERSION;f.record=&r;
    f.motion_scale_x=f.motion_scale_y=f.pre_exposure=1;
    auto size=[&](uint32_t w,uint32_t h){
        r.render_width=r.camera.render_width=w;r.render_height=r.camera.render_height=h;
        f.color={&self,1,VK_FORMAT_R16G16B16A16_SFLOAT,w,h,VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL,0};
        f.depth={&self,2,VK_FORMAT_R32_SFLOAT,w,h,VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL,0};
        f.motion={&self,3,VK_FORMAT_R16G16_SFLOAT,w,h,VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL,0};
        f.output={&self,4,VK_FORMAT_R16G16B16A16_SFLOAT,1920,1080,VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,1};
        f.reactive=f.composition=f.bias_current_color={};
    };
    auto call=[&](){return rsf_srvk_evaluate(&self,&self,&f);};
    auto reject=[&](){unsigned n=xess_calls;CHECK(call()==RSF_BACKEND_ERROR_INVALID_ARGUMENT && n==xess_calls);};
    size(1144,640);CHECK(call()==0 && xess_calls==1);
    f.depth.width--;reject();size(1144,640);f.motion.height--;reject();
    size(1144,640);r.camera.render_height--;reject();
    size(1144,640);f.color.format=VK_FORMAT_R8_UNORM;reject();
    size(1144,640);f.output.layout=VK_IMAGE_LAYOUT_GENERAL;reject();
    size(1144,640);r.session_id=0;reject();r.session_id=1;
    for(auto *m:{&f.reactive,&f.composition,&f.bias_current_color}) {
        *m={&self,5,VK_FORMAT_R8_UNORM,1130,636,VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL,0};reject();*m={};
    }
    size(1144,640);f.reactive={&self,5,VK_FORMAT_R8_UNORM,1144,640,VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL,0};CHECK(call()==0);
    size(1128,632);CHECK(call()==RSF_BACKEND_ERROR_FEATURE_FAILED); // backend range refusal is preserved
    size(1296,728);CHECK(call()==RSF_BACKEND_ERROR_FEATURE_FAILED);
    size(0,640);reject();size(1144,640);r.struct_size=4;reject();r.struct_size=sizeof r;
    f.record=nullptr;reject();f.record=&r;
    self.xess=nullptr;self.dlss=&self;size(1144,640);reject();CHECK(!dlss_calls);
    size(1130,636);CHECK(call()==0 && dlss_calls==1);
    self.dlss=nullptr;size(1144,640);reject();size(1130,636);CHECK(call()==0);
    std::puts("Common SR XeSS active extent admission/dispatch passed; DLSS/FFX strict plan retained");
}
'''
path=out/'contract.cpp';path.write_text(code, encoding="utf-8")
clang='C:/msys64/ucrt64/bin/clang++.exe'
inc=[]
for d in ('runtime/orchestrator/include','sdk/game/include','runtime/contract/include'):
    inc+=['-I',str(root/d)]
subprocess.run([clang,'-std=c++20','-O1','-Wall','-Werror','-DRSF_HAVE_FFX_VK=1','-DRSF_RUNTIME_BUILD',*inc,str(path),'-o',str(out/'contract.exe')],check=True)
subprocess.run([str(out/'contract.exe')],check=True)

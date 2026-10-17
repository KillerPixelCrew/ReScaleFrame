"""Export the pinned upstream shaders with OpenGL bindings for source comparison.

The active port retains eight raw snapshots and transports coordinates instead
of repeatedly filtering old colour. These reference files are not shipped.
"""
from pathlib import Path
import hashlib,json,re,subprocess

root=Path(__file__).resolve().parents[1]
upstream=Path('D:/Coding/VulkanStory-Rewrite')
out=root/'artifacts/upstream-port'
out.mkdir(parents=True,exist_ok=True)
revision=subprocess.check_output(['git','-C',str(upstream),'rev-parse','HEAD'],text=True).strip()
manifest={'repository':'https://github.com/KillerPixelCrew/VulkanStory','revision':revision,'files':{}}
for stem in ['taa-cache','taa-resolve']:
    source=upstream/f'shaders/native/{stem}.glsl'
    original=source.read_text(encoding='utf-8')
    marker='#elif defined(OPTIMUM_FRAGMENT)\nlayout(location = 0) in vec2 texCoord;'
    assert original.count(marker)==1, 'Upstream shader layout changed'
    fragment=original.split(marker,1)[1].rsplit('#endif',1)[0]
    samplers=re.findall(r'OPTIMUM_SAMPLER_SLOT\(sampler2D,\s*(\w+)\);',original)
    block=re.search(r'uniform OptimumProgram\s*\{(.*?)\};',original,re.S).group(1)
    uniforms='\n'.join('uniform '+line.strip() for line in block.splitlines() if line.strip())
    fragment=re.sub(r'optimumTextures2D\[(\w+)\]',r'\1',fragment)
    header=f'#version 330 core\n// VulkanStory-Rewrite {revision}, binding-only reference.\n'
    header+='in vec2 texCoord;\n'+''.join(f'uniform sampler2D {name};\n' for name in samplers)+uniforms+'\n'
    (out/f'{stem}.frag').write_text((header+fragment).rstrip()+'\n',encoding='utf-8')
    manifest['files'][str(source.relative_to(upstream))]=hashlib.sha256(source.read_bytes()).hexdigest()
(root/'taa-upstream.json').write_text(json.dumps(manifest,indent=2)+'\n',encoding='utf-8')
print('Exported the upstream cache and resolve reference at',revision)

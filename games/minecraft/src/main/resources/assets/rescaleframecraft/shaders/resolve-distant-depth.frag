#version 330 core
uniform sampler2D sourceDepth,sourceMotion,distantDepth;
uniform bool useDistantDepth;
uniform mat4 sourceToGuide,distantToGuide;
layout(location=0) out vec4 outMotion;
layout(location=1) out float outDepth;
void main() {
    ivec2 pixel=ivec2(gl_FragCoord.xy),size=textureSize(sourceDepth,0);
    vec2 ndc=gl_FragCoord.xy/vec2(size)*2.0-1.0;
    float depth=texelFetch(sourceDepth,pixel,0).r;
    outDepth=1.0;
    outMotion=texelFetch(sourceMotion,pixel,0);
    if(depth<1.0) {
        vec4 guide=sourceToGuide*vec4(ndc,depth*2.0-1.0,1.0);
        if(guide.w>0.0)outDepth=guide.z/guide.w*0.5+0.5;
    }
    if(useDistantDepth) {
        float lodDepth=texelFetch(distantDepth,pixel,0).r;
        if(lodDepth<1.0) {
            vec4 guide=distantToGuide*vec4(ndc,lodDepth*2.0-1.0,1.0);
            float candidate=guide.z/guide.w*0.5+0.5;
            if(guide.w>0.0&&candidate<outDepth) {outDepth=candidate;outMotion=vec4(0.0);}
        }
    }
    if(outMotion.a>0.0)outMotion.a=outDepth;
    gl_FragDepth=outDepth;
}

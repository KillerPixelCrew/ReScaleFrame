#version 330 core
uniform sampler2D sceneColor,sceneDepth,sceneMotion;
uniform mat4 inverseCurrent,previousViewProjection;
uniform vec3 cameraDelta;
uniform vec2 renderSize,jitterPx;
uniform int resetHistory;
layout(location=0) out vec4 outColor;
layout(location=1) out float outDepth;
layout(location=2) out vec2 outMotion;
void main() {
    ivec2 p=ivec2(gl_FragCoord.x,renderSize.y-gl_FragCoord.y);
    vec2 centre=vec2(p)+0.5,ndc=centre/renderSize*2.0-1.0;
    float depth=texelFetch(sceneDepth,p,0).r;
    vec4 motion=texelFetch(sceneMotion,p,0);
    vec4 h=inverseCurrent*vec4(ndc,depth*2.0-1.0,1.0),previous;
    if(depth>=1.0) {
        vec4 nearH=inverseCurrent*vec4(ndc,-1.0,1.0);
        vec3 direction=h.xyz*nearH.w-nearH.xyz*h.w;
        if((h.w<0.0)!=(nearH.w<0.0))direction=-direction;
        previous=previousViewProjection*vec4(direction,0.0);
    } else previous=previousViewProjection*vec4(h.xyz/h.w+cameraDelta,1.0);
    vec2 delta=vec2(0.0);
    if(previous.w>1e-6)delta=(previous.xy/previous.w*0.5+0.5)*renderSize-(centre-jitterPx);
    if(motion.a>0.0&&abs(motion.a-depth)<=max(2e-4,8e-4*depth))delta=motion.rg;
    if(resetHistory!=0||any(isnan(delta))||any(isinf(delta)))delta=vec2(0.0);
    // Framework input: current minus previous UV, DX top-left origin, unjittered.
    // The native normalizer converts this to vendor previous-minus-current pixels.
    outMotion=vec2(-delta.x,delta.y);
    outDepth=depth;outColor=texelFetch(sceneColor,p,0);
}

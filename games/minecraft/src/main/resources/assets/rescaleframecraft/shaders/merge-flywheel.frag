#version 330 core
uniform sampler2D capturedMotion;
uniform sampler2D capturedDepth;
uniform sampler2D capturedPlane;
layout(location=0) out vec4 outMotion;
layout(location=1) out float outDepth;
layout(location=2) out vec2 outPlane;
void main() {
    ivec2 pixel=ivec2(gl_FragCoord.xy);
    vec4 motion=texelFetch(capturedMotion,pixel,0);
    if(motion.w<0.0)discard;
    outMotion=motion;
    outDepth=texelFetch(capturedDepth,pixel,0).r;
    outPlane=texelFetch(capturedPlane,pixel,0).rg;
    gl_FragDepth=outDepth;
}

#version 330 core
uniform sampler2D capturedMotion;
uniform sampler2D capturedDepth;
uniform sampler2D capturedPlane;
uniform usampler2D capturedActors;
uniform usamplerBuffer actorValidity;
layout(location=0) out vec4 outMotion;
layout(location=1) out float outDepth;
layout(location=2) out vec2 outPlane;
void main() {
    ivec2 pixel=ivec2(gl_FragCoord.xy);
    vec4 motion=texelFetch(capturedMotion,pixel,0);
    if(motion.w<0.0)discard;
    uint actor=texelFetch(capturedActors,pixel,0).r;
    bool invalid=actor>0u&&(actor>=uint(textureSize(actorValidity))||texelFetch(actorValidity,int(actor)).r==0u);
    if(invalid)motion=vec4(0.0,0.0,1.0,motion.a);
    outMotion=motion;
    outDepth=texelFetch(capturedDepth,pixel,0).r;
    outPlane=invalid?vec2(0.0):texelFetch(capturedPlane,pixel,0).rg;
    gl_FragDepth=outDepth;
}

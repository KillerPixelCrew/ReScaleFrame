#version 330 core
uniform sampler2D sourceDepth;
uniform int reversedDepth;
in vec2 texCoord;
layout(location=0) out vec4 outMotion;
layout(location=1) out float outDepth;
void main() {
    float depth=texelFetch(sourceDepth,ivec2(gl_FragCoord.xy),0).r;
    outDepth=reversedDepth!=0?1.0-depth:depth;
    outMotion=vec4(0.0);
    gl_FragDepth=outDepth;
}

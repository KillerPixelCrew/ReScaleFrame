#version 330 core
uniform sampler2D sourceDepth;
uniform vec2 destinationSize;
void main() {
    ivec2 size=textureSize(sourceDepth,0);
    ivec2 pixel=clamp(ivec2(gl_FragCoord.xy*vec2(size)/destinationSize),ivec2(0),size-1);
    gl_FragDepth=texelFetch(sourceDepth,pixel,0).r;
}

#version 330 core
layout(location=0) in vec3 Position;
layout(location=1) in vec2 UV;
layout(location=2) in vec4 Color;
uniform mat4 currentProjection,currentModelView;
uniform int reversedDepth,zeroToOne;
uniform mat4 previousViewProjection;
out vec4 previousClip;
out vec2 texCoord;
out vec2 alphaReactive;
uniform int validHistory;
void main() {
    vec4 clip=currentProjection*currentModelView*vec4(Position,1.0);
    if(zeroToOne!=0)clip.z=2.0*clip.z-clip.w;
    if(reversedDepth!=0)clip.z=-clip.z;
    gl_Position=clip;
    previousClip=previousViewProjection*vec4(Position,1.0);
    texCoord=UV;
    alphaReactive=vec2(Color.a,validHistory!=0?0.0:1.0);
}

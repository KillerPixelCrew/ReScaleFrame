#version 330 core
layout(location=0) in vec3 Position;
layout(location=1) in vec3 PreviousPosition;
layout(location=2) in vec2 UV;
layout(location=3) in vec2 AlphaReactive;
uniform mat4 currentViewProjection;
uniform mat4 previousViewProjection;
uniform mat4 textureMatrix;
out vec4 previousClip;
out vec2 texCoord;
out vec2 alphaReactive;
void main() {
    gl_Position=currentViewProjection*vec4(Position,1.0);
    previousClip=previousViewProjection*vec4(PreviousPosition,1.0);
    texCoord=(textureMatrix*vec4(UV,0.0,1.0)).xy;
    alphaReactive=AlphaReactive;
}

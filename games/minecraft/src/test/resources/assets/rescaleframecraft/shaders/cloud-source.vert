#version 330 core
layout(location=0) in vec3 Position;
layout(location=1) in vec4 Color;
uniform mat4 projection,modelView;
out vec4 color;
void main() { gl_Position=projection*modelView*vec4(Position,1.0);color=Color; }

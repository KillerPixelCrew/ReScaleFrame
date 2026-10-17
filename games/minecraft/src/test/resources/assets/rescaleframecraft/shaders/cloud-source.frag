#version 330 core
in vec4 color;
layout(location=0) out vec4 outColor;
void main() { outColor=color*vec4(0.8,0.8,0.8,0.8); }

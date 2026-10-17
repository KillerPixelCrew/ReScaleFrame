#version 330 core
uniform sampler2D colorTexture;
uniform int textured;
uniform vec2 renderSize;
uniform vec2 jitterPx;
in vec4 previousClip;
in vec2 texCoord;
in vec2 alphaReactive;
layout(location=0) out vec4 outMotion;
layout(location=1) out float outDepth;
void main() {
    float alpha=alphaReactive.x*(textured!=0?texture(colorTexture,texCoord).a:1.0);
    if(alpha<0.01)discard;
    float reactive=max(alphaReactive.y,1.0-alpha);
    if(previousClip.w<=1e-6) {
        outMotion=vec4(0.0,0.0,1.0,0.0);
    } else {
        vec2 previousPixel=(previousClip.xy/previousClip.w*0.5+0.5)*renderSize;
        outMotion=vec4(previousPixel-(gl_FragCoord.xy-jitterPx),reactive,gl_FragCoord.z);
    }
    outDepth=gl_FragCoord.z;
}

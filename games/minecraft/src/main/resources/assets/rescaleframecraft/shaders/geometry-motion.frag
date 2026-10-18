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
layout(location=2) out vec2 outPlane;
void main() {
    vec2 oldPixel=(previousClip.xy/max(previousClip.w,1e-6)*0.5+0.5)*renderSize;
    float oldInverse=1.0/max(previousClip.w,1e-6);
    vec2 dx=dFdx(oldPixel),dy=dFdy(oldPixel);
    vec2 dz=vec2(dFdx(oldInverse),dFdy(oldInverse));
    float determinant=dx.x*dy.y-dx.y*dy.x;
    bool invertible=determinant>1e-8;
    outPlane=invertible?vec2(dz.x*dy.y-dz.y*dx.y,dx.x*dz.y-dy.x*dz.x)/determinant:vec2(0.0);
    if(any(isnan(outPlane))||any(isinf(outPlane))||any(greaterThan(abs(outPlane),vec2(65504.0)))) {outPlane=vec2(0.0);invertible=false;}
    float alpha=alphaReactive.x*(textured!=0?texture(colorTexture,texCoord).a:1.0);
    if(alpha<0.01)discard;
    float reactive=max(alphaReactive.y,1.0-alpha);
    if(!invertible)reactive=1.0;
    if(previousClip.w<=1e-6) {
        outMotion=vec4(0.0,0.0,1.0,0.0);
    } else {
        vec2 previousPixel=(previousClip.xy/previousClip.w*0.5+0.5)*renderSize;
        // Reciprocal previous view depth fits the existing half-float channel.
        // Visibility must follow the old moving surface, not today's position.
        outMotion=vec4(previousPixel-(gl_FragCoord.xy-jitterPx),reactive,1.0/previousClip.w);
    }
    outDepth=gl_FragCoord.z;
}

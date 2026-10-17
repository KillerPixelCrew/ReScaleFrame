#version 330 core
in vec2 texCoord;
uniform sampler2D accumulation,currentColor,currentDepth,currentMotion;
uniform vec2 renderSize,jitterPx;
layout(location=0) out vec4 outColor;
void main() {
    ivec2 pixel=ivec2(gl_FragCoord.xy),closest=clamp(ivec2(gl_FragCoord.xy+jitterPx),ivec2(0),ivec2(renderSize)-1);
    float nearest=texelFetch(currentDepth,closest,0).r;
    for(int y=-1;y<=1;y++)for(int x=-1;x<=1;x++) {
        ivec2 tap=clamp(ivec2(gl_FragCoord.xy+jitterPx)+ivec2(x,y),ivec2(0),ivec2(renderSize)-1);
        float depth=texelFetch(currentDepth,tap,0).r;
        if(depth<nearest) { nearest=depth;closest=tap; }
    }
    vec4 sum=texelFetch(accumulation,pixel,0);
    vec3 mean=sum.rgb/max(sum.a,1.0);
    vec2 p=gl_FragCoord.xy+jitterPx-0.5;ivec2 base=ivec2(floor(p));vec2 f=fract(p);ivec2 maximum=ivec2(renderSize)-1;
    vec3 current=mix(mix(texelFetch(currentColor,clamp(base,ivec2(0),maximum),0).rgb,texelFetch(currentColor,clamp(base+ivec2(1,0),ivec2(0),maximum),0).rgb,f.x),
                     mix(texelFetch(currentColor,clamp(base+ivec2(0,1),ivec2(0),maximum),0).rgb,texelFetch(currentColor,clamp(base+ivec2(1),ivec2(0),maximum),0).rgb,f.x),f.y);
    float reactive=clamp(texelFetch(currentMotion,closest,0).b,0.0,1.0);
    outColor=vec4(mix(mean,current,reactive),1.0);
}

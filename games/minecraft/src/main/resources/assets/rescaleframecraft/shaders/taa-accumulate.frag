#version 330 core
// Eight-raw-sample window and homogeneous sky reprojection derived from
// VulkanStory-Rewrite 0bbcf237ba583e3c02aa19824f3440df145bcaab.
// Retain raw raster snapshots; transport coordinates, never resampled colour.
in vec2 texCoord;
uniform sampler2D accumulation,coordinateMap,currentColor,currentDepth,currentMotion,previousColor,previousDepth;
uniform sampler2D currentPlane;
uniform mat4 inverseCurrent,currentView,previousViewProjection,inversePrevious,previousView;
uniform vec2 renderSize,currentJitter,previousJitter;
uniform vec3 cameraDelta;
uniform int stage;
layout(location=0) out vec4 outAccumulation;
layout(location=1) out vec4 outCoordinate;
bool finite4(vec4 value) { return !any(isnan(value))&&!any(isinf(value)); }
ivec2 supportMin(vec2 raster) { return ivec2(floor(raster-0.5))-1; }
ivec2 supportMax(vec2 raster) {
    vec2 p=raster-0.5;vec2 f=fract(p);
    return ivec2(floor(p))+ivec2(f.x>0.0?2:1,f.y>0.0?2:1);
}
vec4 rawSample(sampler2D image,vec2 pixel) {
    vec2 p=pixel-0.5;ivec2 base=ivec2(floor(p));vec2 f=fract(p);ivec2 maximum=ivec2(renderSize)-1;
    return mix(mix(texelFetch(image,clamp(base,ivec2(0),maximum),0),texelFetch(image,clamp(base+ivec2(1,0),ivec2(0),maximum),0),f.x),
               mix(texelFetch(image,clamp(base+ivec2(0,1),ivec2(0),maximum),0),texelFetch(image,clamp(base+ivec2(1),ivec2(0),maximum),0),f.x),f.y);
}
float previousLinearDepth(ivec2 pixel,float depth) {
    vec4 h=inversePrevious*vec4((vec2(pixel)+0.5)/renderSize*2.0-1.0,depth*2.0-1.0,1.0);
    return abs(h.w)>1e-6?-(previousView*vec4(h.xyz/h.w,1.0)).z:0.0;
}
float currentLinearDepth(ivec2 pixel,float depth) {
    vec4 h=inverseCurrent*vec4((vec2(pixel)+0.5)/renderSize*2.0-1.0,depth*2.0-1.0,1.0);
    return abs(h.w)>1e-6?-(currentView*vec4(h.xyz/h.w,1.0)).z:0.0;
}
bool historyColorVisible(vec2 raster,bool sky,float expectedDepth) {
    vec2 position=raster-0.5;ivec2 base=ivec2(floor(position));vec2 f=fract(position);
    for(int y=0;y<2;y++)for(int x=0;x<2;x++) {
        float weight=(x==0?1.0-f.x:f.x)*(y==0?1.0-f.y:f.y);
        if(weight==0.0)continue;
        ivec2 tap=clamp(base+ivec2(x,y),ivec2(0),ivec2(renderSize)-1);
        float depth=texelFetch(previousDepth,tap,0).r;
        // The depth that validated the motion is not necessarily the depth of
        // the colour taps. Never import a nearer occluder's RGB into a cloud or
        // sky history just because the right surface exists beside it.
        if(sky) { if(depth<1.0)return false; }
        else if(depth<1.0&&previousLinearDepth(tap,depth)<expectedDepth-(0.5+0.08*abs(expectedDepth)))return false;
    }
    return true;
}
float halfUnit(float value) {return max(exp2(floor(log2(max(abs(value),5.96046448e-8)))-10.0),5.96046448e-8);}
bool movingSurfaceVisible(vec2 raster,vec2 reference,vec4 motion,vec2 plane) {
    if(any(isnan(plane))||any(isinf(plane)))return false;
    vec2 p=raster-0.5;ivec2 base=ivec2(floor(p));vec2 f=fract(p);
    for(int y=0;y<2;y++)for(int x=0;x<2;x++) {
        float weight=(x==0?1.0-f.x:f.x)*(y==0?1.0-f.y:f.y);if(weight==0.0)continue;
        ivec2 tap=clamp(base+ivec2(x,y),ivec2(0),ivec2(renderSize)-1);
        float depth=texelFetch(previousDepth,tap,0).r;if(depth>=1.0)return false;
        vec2 offset=vec2(tap)+0.5-reference;
        float expected=motion.a+dot(plane,offset);if(expected<=0.0)return false;
        float linear=previousLinearDepth(tap,depth);if(linear<=0.0)return false;
        // Half-float depth/slope/motion rounding plus float depth unprojection.
        // This is a storage error bound, not a fixed world-distance acceptance radius.
        float error=halfUnit(motion.a)+dot(vec2(halfUnit(plane.x),halfUnit(plane.y)),abs(offset))+
            dot(abs(plane),vec2(halfUnit(motion.x),halfUnit(motion.y)))+
            (abs(inversePrevious[2].w)+abs(inversePrevious[3].w))*4.76837158e-7;
        if(abs(1.0/linear-expected)>error)return false;
    }
    return true;
}
void main() {
    ivec2 pixel=ivec2(gl_FragCoord.xy);vec2 centre=vec2(pixel)+0.5;
    if(stage==0) {
        vec4 color=rawSample(currentColor,centre+currentJitter);
        outAccumulation=vec4(finite4(color)?max(color.rgb,vec3(0.0)):vec3(0.0),1.0);
        ivec2 raster=clamp(ivec2(centre+currentJitter),ivec2(0),ivec2(renderSize)-1),closest=raster;
        float nearest=texelFetch(currentDepth,raster,0).r;
        // Use the exact nonzero source support of the reconstructed 3x3
        // output neighbourhood. It shifts with jitter; a symmetric 5x5 box
        // can borrow a vine outside that support to move cloud colour.
        ivec2 minimum=supportMin(centre+currentJitter),maximum=supportMax(centre+currentJitter);
        for(int y=0;y<4;y++)for(int x=0;x<4;x++) {
            ivec2 candidate=minimum+ivec2(x,y);if(any(greaterThan(candidate,maximum)))continue;
            ivec2 tap=clamp(candidate,ivec2(0),ivec2(renderSize)-1);
            float depth=texelFetch(currentDepth,tap,0).r;
            if(depth<nearest) { nearest=depth;closest=tap; }
        }
        outCoordinate=vec4(centre,1.0,nearest>=1.0?0.0:currentLinearDepth(closest,nearest));return;
    }
    outAccumulation=texelFetch(accumulation,pixel,0);
    vec4 coordinate=texelFetch(coordinateMap,pixel,0);outCoordinate=vec4(coordinate.xy,0.0,0.0);
    if(coordinate.z==0.0)return;
    vec2 raster=coordinate.xy+currentJitter;
    ivec2 currentPixel=clamp(ivec2(raster),ivec2(0),ivec2(renderSize)-1),closest=currentPixel;
    bool sky=coordinate.w==0.0;
    float closestDepth=texelFetch(currentDepth,currentPixel,0).r;
    if(sky) {
        if(closestDepth<1.0)return;
    } else {
        float best=1e30;bool found=false;
        ivec2 minimum=supportMin(raster),maximum=supportMax(raster);
        for(int y=0;y<4;y++)for(int x=0;x<4;x++) {
            ivec2 candidate=minimum+ivec2(x,y);if(any(greaterThan(candidate,maximum)))continue;
            ivec2 tap=clamp(candidate,ivec2(0),ivec2(renderSize)-1);
            float depth=texelFetch(currentDepth,tap,0).r;
            if(depth>=1.0)continue;
            float error=abs(currentLinearDepth(tap,depth)-coordinate.w);
            float tolerance=0.5+0.08*abs(coordinate.w);
            if(error>tolerance)continue;
            vec2 offset=vec2(tap)+0.5-raster;
            float score=dot(offset,offset)+error/tolerance*0.1;
            if(score<best) { best=score;closest=tap;closestDepth=depth;found=true; }
        }
        if(!found)return;
    }
    vec2 closestCentre=vec2(closest)+0.5;
    vec2 ndc=closestCentre/renderSize*2.0-1.0;
    vec4 h=inverseCurrent*vec4(ndc,closestDepth*2.0-1.0,1.0);
    vec4 motion=texelFetch(currentMotion,closest,0);
    // A replaced mesh has no previous geometry correspondence. Do not carry
    // older colour through that broken link in the motion chain.
    if(motion.b>=1.0)return;
    vec4 projected;
    float expectedDepth=0.0;
    if(sky) {
        vec4 nearH=inverseCurrent*vec4(ndc,-1.0,1.0);
        vec3 direction=h.xyz*nearH.w-nearH.xyz*h.w;
        if((h.w<0.0)!=(nearH.w<0.0))direction=-direction;
        projected=previousViewProjection*vec4(direction,0.0);
    } else {
        if(abs(h.w)<1e-6)return;
        vec3 previousWorld=h.xyz/h.w+cameraDelta;
        projected=previousViewProjection*vec4(previousWorld,1.0);
        expectedDepth=-(previousView*vec4(previousWorld,1.0)).z;
    }
    if(!finite4(projected)||projected.w<=1e-6||!finite4(motion))return;
    vec2 delta=(projected.xy/projected.w*0.5+0.5)*renderSize-(closestCentre-currentJitter);
    bool written=motion.a>0.0;
    if(written) {delta=motion.rg;if(!sky)expectedDepth=1.0/motion.a;}
    vec2 previousCentre=coordinate.xy+delta;
    vec2 previousRaster=previousCentre+previousJitter;
    if(any(lessThan(previousRaster,vec2(0.5)))||any(greaterThan(previousRaster,renderSize-0.5)))return;
    vec2 previousClosest=closestCentre-currentJitter+delta+previousJitter;
    ivec2 oldPixel=clamp(ivec2(previousClosest),ivec2(0),ivec2(renderSize)-1);
    if(sky) {
        // The corresponding sky sample must be sky. An unrelated neighbouring
        // foreground sample must not invalidate an otherwise stable sky age.
        vec2 samplePosition=previousRaster-0.5;ivec2 base=ivec2(floor(samplePosition));vec2 f=fract(samplePosition);
        for(int y=0;y<2;y++)for(int x=0;x<2;x++) {
            float weight=(x==0?1.0-f.x:f.x)*(y==0?1.0-f.y:f.y);
            ivec2 tap=clamp(base+ivec2(x,y),ivec2(0),ivec2(renderSize)-1);
            if(weight>0.0&&texelFetch(previousDepth,tap,0).r<1.0)return;
        }
    } else {
        float error=1e30;
        float matchedDepth=expectedDepth;
        ivec2 minimum=supportMin(previousRaster),maximum=supportMax(previousRaster);
        for(int y=0;y<4;y++)for(int x=0;x<4;x++) {
            ivec2 candidate=minimum+ivec2(x,y);if(any(greaterThan(candidate,maximum)))continue;
            ivec2 tap=clamp(candidate,ivec2(0),ivec2(renderSize)-1);
            float depth=texelFetch(previousDepth,tap,0).r;
            if(depth<1.0) {
                float linear=previousLinearDepth(tap,depth),difference=abs(linear-expectedDepth);
                if(difference<error) { error=difference;matchedDepth=linear; }
            }
        }
        if(error>0.5+0.08*abs(expectedDepth))return;
        expectedDepth=matchedDepth;
    }
    if(written&&!sky) {
        if(!movingSurfaceVisible(previousRaster,previousClosest,motion,texelFetch(currentPlane,closest,0).rg))return;
    } else if(!historyColorVisible(previousRaster,sky,expectedDepth))return;
    vec4 color=rawSample(previousColor,previousRaster);
    if(!finite4(color))return;
    outAccumulation+=vec4(max(color.rgb,vec3(0.0)),1.0);
    outCoordinate=vec4(previousCentre,1.0,sky?0.0:expectedDepth);
}

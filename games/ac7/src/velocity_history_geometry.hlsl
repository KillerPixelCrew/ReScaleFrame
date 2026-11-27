// SPDX-License-Identifier: MIT
struct V{float4 f0:SV_Position0;float4 f1:RSFKEY0;float4 f2:RSFCENTRE0;float4 f3:RSFOLD0;float4 f4:RSFLOOKUP0;};struct O{float4 f0:SV_Position0;float4 f1:RSFKEY0;float4 f2:RSFCENTRE0;float4 f3:RSFOLD0;float4 f4:RSFLOOKUP0;float4 current:RSFCUR0;float4 previous:RSFPREV0;};

uint decode_tag(float value) {uint bits=asuint(value);return (bits&0x1fffffff)|((bits>>2)&0x20000000);}
float encode_tag(uint value) {return asfloat(0x20000000|(value&0x1fffffff)|((value&0x20000000)<<2));}
bool endpoint(float4 key,out float4 position,out bool ambiguous) {
    uint4 bits=asuint(key);uint hash=key_hash(bits);bool found=false;position=0;ambiguous=false;
    [loop]for(uint probe=0;probe<HistoryProbeCount;probe++) {
        uint address=history_address(hash,probe,HistoryMask);uint marker=History.Load(address+HistoryMarkerOffset);
        if(marker==0)break;if(marker!=hash || any(History.Load4(address+HistoryKeyOffset)!=bits))continue;
        float4 candidate=asfloat(History.Load4(address+HistoryClipOffset));if(!all(isfinite(candidate)) || candidate.w<=0)continue;
        if(!found) {position=candidate;found=true;}
        else if(any(asuint(position)!=asuint(candidate)))ambiguous=true;
    }
    return found;
}
float4 previous_position(float4 current,float4 key,float3 old) {
    float4 unjittered=current;unjittered.xy-=Jitter.xy*current.w;
    float4 fallback=mul(unjittered,ClipToPrevious);fallback.xy+=Jitter.zw*fallback.w;
    if(HasHistory==0 || key.w==3)return fallback;
    if(key.w==4 || key.w==5) {
        bool left=false,right=false,bad_left=false,bad_right=false;float low=-1,high=2;float4 a=fallback,b=fallback;
        uint first=decode_tag(key.x),second=decode_tag(key.y);
        if(first>>28!=0) {
            float4 start=key;start.y=encode_tag((second&~0x3ffffff)|(first&0x3ffffff));start.z=0;
            left=endpoint(start,a,bad_left);if(left)low=0;
            float4 end=key;end.x=encode_tag((first&~0x3ffffff)|(second&0x3ffffff));end.z=0;
            right=endpoint(end,b,bad_right);if(right)high=1;
            if(old.x==0)return left && !bad_left ? a : fallback;
            if(old.x==1)return right && !bad_right ? b : fallback;
            // A head point has no prior segment. Its own rendered endpoint still has history.
            if((first&0x3ffffff)==(second&0x3ffffff))return left && !bad_left ? a : fallback;
        }
        uint bin=uint(key.z);
        [loop]for(uint distance=0;distance<=128;distance++) {
            if(left && right && (first>>28==0 || (low>=float(int(bin)-int(distance))/128 && high<=float(bin+distance)/128)))break;
            [unroll]for(uint side=0;side<2;side++) {
                int target=int(bin)+(side==0 ? -int(distance) : int(distance));
                if(target<0 || target>128 || (side==0 && left && low>=float(target+1)/128) || (side==1 && right && high<float(target)/128))continue;
                float4 query=key;query.z=float(target);uint4 bits=asuint(query);uint hash=key_hash(bits);
                [loop]for(uint probe=0;probe<HistoryProbeCount;probe++) {
                    uint address=history_address(hash,probe,HistoryMask);uint marker=History.Load(address+HistoryMarkerOffset);
                    if(marker==0)break;if(marker!=hash || any(History.Load4(address+HistoryKeyOffset)!=bits))continue;
                    float4 clip=asfloat(History.Load4(address+HistoryClipOffset));float u=asfloat(History.Load(address+HistoryCentreOffset));
                    if(!all(isfinite(clip)) || clip.w<=0 || !isfinite(u))continue;
                    if(u<=old.x && (!left || u>low)) {left=true;low=u;a=clip;bad_left=false;}
                    else if(left && u==low && any(asuint(a)!=asuint(clip)))bad_left=true;
                    if(u>=old.x && (!right || u<high)) {right=true;high=u;b=clip;bad_right=false;}
                    else if(right && u==high && any(asuint(b)!=asuint(clip)))bad_right=true;
                }
            }
        }
        if(!left || !right || bad_left || bad_right)return fallback;
        return high==low ? a : lerp(a,b,(old.x-low)/(high-low));
    }
    return lookup_previous(asuint(key),fallback,old,key.w==1);
}
[maxvertexcount(3)]void main(triangle V input[3],inout TriangleStream<O> stream) {
    [unroll]for(uint j=0;j<3;j++) {O output;
output.f0=input[j].f0;output.f1=input[j].f1;output.f2=input[j].f2;output.f3=input[j].f3;output.f4=input[j].f4;output.current=input[j].f0;output.previous=previous_position(input[j].f0,input[j].f4,input[j].f3.xyz);stream.Append(output);}stream.RestartStrip();}

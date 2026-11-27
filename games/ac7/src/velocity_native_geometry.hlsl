// SPDX-License-Identifier: MIT
struct O {float4 key:RSFKEY0;float4 centre:RSFCENTRE0;float4 old:RSFOLD0;float4 previous:RSFPREV0;float4 current:RSFCUR0;};
O main(float4 p:RSFCLIP0,float4 a:RSFA0,float4 b:RSFA1,float4 c:RSFA2,
       float4 la:RSFB0,float4 lb:RSFB1,float4 lc:RSFB2,
       float4 centre:RSFCENTRE0,float4 old:RSFOLD0,uint ordinal:RSFORDINAL0) {
 O o;o.key=asfloat(uint4(triangle_key(a,b,c),ordinal));o.centre=centre;o.old=old;o.current=p;
 float4 unjittered=p;unjittered.xy-=Jitter.xy*p.w;
 o.previous=mul(unjittered,ClipToPrevious);o.previous.xy+=Jitter.zw*o.previous.w;
 if(HasHistory!=0 && la.w!=3 && lb.w!=3 && lc.w!=3)
  o.previous=lookup_previous(uint4(triangle_key(la,lb,lc),ordinal),o.previous,old.xyz,la.w==1);
 return o;
}

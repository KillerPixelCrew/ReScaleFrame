// SPDX-License-Identifier: MIT
struct O {float4 key:RSFKEY0;float4 centre:RSFCENTRE0;float4 old:RSFOLD0;float4 current:RSFCUR0;float4 previous:RSFPREV0;};
float4 previous_corner(float4 current,uint3 patch,uint corner,float3 old,bool sprite,bool valid) {
 float4 p=current;p.xy-=Jitter.xy*p.w;float4 fallback=mul(p,ClipToPrevious);fallback.xy+=Jitter.zw*fallback.w;
 if(HasHistory==0 || !valid)return fallback;
 return lookup_previous(uint4(patch,asuint(float(corner))),fallback,old,sprite);
}
O main(float3 domain:RSFDOMAIN0,float4 p0:RSFCORNER0,float4 p1:RSFCORNER1,float4 p2:RSFCORNER2,
 float4 a:RSFA0,float4 b:RSFA1,float4 c:RSFA2,float4 la:RSFB0,float4 lb:RSFB1,float4 lc:RSFB2,
 float4 centre:RSFCENTRE0,float4 old:RSFOLD0) {
 O o;uint3 now=triangle_key(a,b,c),prior=triangle_key(la,lb,lc);
 uint corner=domain.x==1 ? 1 : domain.y==1 ? 2 : domain.z==1 ? 3 : 0;
 o.key=asfloat(uint4(now,asuint(float(corner))));o.centre=centre;o.old=old;
 o.current=p0*domain.x+p1*domain.y+p2*domain.z;
 bool valid=la.w!=3 && lb.w!=3 && lc.w!=3;
 o.previous=previous_corner(p0,prior,1,old.xyz,la.w==1,valid)*domain.x+
  previous_corner(p1,prior,2,old.xyz,la.w==1,valid)*domain.y+
  previous_corner(p2,prior,3,old.xyz,la.w==1,valid)*domain.z;
 return o;
}

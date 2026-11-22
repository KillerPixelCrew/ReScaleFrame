// SPDX-License-Identifier: MIT
ByteAddressBuffer Source:register(t0);ByteAddressBuffer Arguments:register(t1);
RWByteAddressBuffer Destination:register(u0);
cbuffer Parameters:register(b0) {uint Count;uint Mask;uint Mode;uint WorkWidth;};
void insert(uint4 clip,uint4 key,uint3 centre) {
    float4 position=asfloat(clip);if(!all(isfinite(position)) || position.w<=0)return;
    uint hash=key_hash(key);
    [loop]for(uint probe=0;probe<HistoryProbeCount;probe++) {
        uint address=history_address(hash,probe,Mask);uint old;
        Destination.InterlockedCompareExchange(address+HistoryMarkerOffset,0,0xffffffff,old);
        if(old==0) {
            Destination.Store4(address+HistoryKeyOffset,key);Destination.Store4(address+HistoryClipOffset,clip);
            Destination.Store3(address+HistoryCentreOffset,centre);Destination.Store(address+HistoryPaddingOffset,0);
            DeviceMemoryBarrier();Destination.Store(address+HistoryMarkerOffset,hash);return;
        }
        if(old==hash && all(Destination.Load4(address+HistoryKeyOffset)==key) &&
           all(Destination.Load4(address+HistoryClipOffset)==clip) && all(Destination.Load3(address+HistoryCentreOffset)==centre))return;
    }
}
[numthreads(64,1,1)]void main(uint3 thread:SV_DispatchThreadID) {
    uint count=WorkWidth ? Count : min(Count,Arguments.Load(32));
    uint width=WorkWidth ? WorkWidth : Arguments.Load(36);
    uint index=thread.x+thread.y*width*64;
    if(index>=count)return;uint offset=index*HistoryRecordBytes;
    if(Mode!=0 && Source.Load(offset+HistoryMarkerOffset)==0)return;
    uint4 clip=Source.Load4(offset+(Mode==0 ? 0 : HistoryClipOffset));
    uint4 key=Source.Load4(offset+(Mode==0 ? 16 : HistoryKeyOffset));
    uint3 centre=Source.Load3(offset+(Mode==0 ? 32 : HistoryCentreOffset));insert(clip,key,centre);
    uint a=(key.x&0x1fffffff)|((key.x>>2)&0x20000000);
    uint b=(key.y&0x1fffffff)|((key.y>>2)&0x20000000);
    if((key.w==asuint(4.f) || key.w==asuint(5.f)) && key.z==0 && centre.x==0 && a>>28!=0) {
        b=(b&~0x3ffffff)|(a&0x3ffffff);
        key.y=0x20000000|(b&0x1fffffff)|((b&0x20000000)<<2);insert(clip,key,centre);
    }
}

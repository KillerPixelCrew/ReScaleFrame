// SPDX-License-Identifier: MIT
// The linking stage supplies the history SRV slot.
ByteAddressBuffer History : register(RSF_HISTORY_SLOT);
cbuffer Parameters : register(b13) {
    float4 Rect; float4 Jitter; float Cutoff; uint HasHistory; uint HistoryMask; uint Padding;
    row_major float4x4 ClipToPrevious;
};
uint3 triangle_key(float4 a,float4 b,float4 c) {
 uint3 hash=uint3(2166136261,2246822519,3266489917);
 [unroll]for(uint v=0;v<3;v++) {
  uint4 bits=asuint(v==0 ? a : v==1 ? b : c);
  [unroll]for(uint j=0;j<4;j++)hash=(hash^bits[j])*uint3(16777619,3266489917,668265263);
 }
 return hash;
}

float4 lookup_previous(uint4 key, float4 fallback, float3 old, bool sprite) {
    uint hash = key_hash(key);
    bool found = false, ambiguous = false;
    float distance = 3.402823e38;
    float4 selected = fallback;
    [loop]for (uint probe = 0; probe < HistoryProbeCount; probe++) {
        uint address = history_address(hash, probe, HistoryMask);
        uint marker = History.Load(address + HistoryMarkerOffset);
        if (marker == 0) break;
        if (marker != hash || any(History.Load4(address + HistoryKeyOffset) != key)) continue;
        float4 candidate = asfloat(History.Load4(address + HistoryClipOffset));
        if (!all(isfinite(candidate)) || candidate.w <= 0) continue;
        float3 centre = asfloat(History.Load3(address + HistoryCentreOffset));
        float d = sprite ? dot(centre - old, centre - old) : 0;
        if (!found || d < distance) {
            found = true; distance = d; selected = candidate; ambiguous = false;
        } else if (d == distance && any(asuint(candidate) != asuint(selected))) {
            ambiguous = true;
        }
    }
    return found && !ambiguous ? selected : fallback;
}

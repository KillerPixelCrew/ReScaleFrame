// SPDX-License-Identifier: MIT
// Hash-table records written to the raw UAV and read by each velocity stage.
static const uint HistoryRecordBytes = 64;
static const uint HistoryProbeCount = 32;
static const uint HistoryMarkerOffset = 0;
static const uint HistoryKeyOffset = 4;
static const uint HistoryClipOffset = 20;
static const uint HistoryCentreOffset = 36;
static const uint HistoryPaddingOffset = 48;

uint key_hash(uint4 key) {
    uint value=2166136261;
    [unroll]for(uint i=0;i<4;i++)value=(value^key[i])*16777619;
    value^=value>>16;return max(1,value&0x7fffffff);
}
uint history_address(uint hash, uint probe, uint mask) {
    return ((hash + probe) & mask) * HistoryRecordBytes;
}

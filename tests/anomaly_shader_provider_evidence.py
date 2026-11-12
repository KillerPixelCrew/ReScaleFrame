"""Read-only includer evidence for both exact Anomaly executables; never deploys."""
import sys, struct
sys.path.insert(0, str(__import__('pathlib').Path(__file__).resolve().parent))
import anomaly_geometry_evidence as g
for build,setup,table,open_rva,close_rva in (
    ('standard',0x83b9ce,0xb67398,0x8377a0,0x837320),
    ('avx',0x832cce,0xb54508,0x82e9f0,0x82e570)):
    g.select(build)
    pe=g.PE()
    lea=setup+6
    assert pe.read(lea,3)==bytes.fromhex('488d05')
    found=lea+7+struct.unpack('<i',pe.read(lea+3,4))[0]
    assert found==table
    entries=struct.unpack('<QQ',pe.read(table,16))
    assert entries==(pe.base+open_rva,pe.base+close_rva)
    assert pe.read(open_rva,9)==bytes.fromhex('40534881ec40020000')
    assert pe.read(close_rva,8)==bytes.fromhex('4883ec284885d274')
    print(build, 'includer table', hex(table), 'Open', hex(open_rva), 'Close', hex(close_rva), 'verified')

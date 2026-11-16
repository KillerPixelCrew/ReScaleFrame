# SPDX-License-Identifier: GPL-3.0-only
"""Bounded public-API actions for a caller-owned, readiness-verified Runtime module.

Usage: PID MODULE_BASE RUNTIME_DLL ACTION SHA256 [PATCH_JSON for apply].
The caller owns process selection/readiness; this helper never searches for a game.
Importing this module defines layouts/tests only and never opens a process.
"""
import ctypes as c
import hashlib
import json
import struct
import sys
from pathlib import Path

U32, I32, U64, F32 = c.c_uint32, c.c_int32, c.c_uint64, c.c_float

class InputRoutes(c.Structure):
    _fields_ = [('struct_size', U32), ('version', U32)] + [(name, U64) for name in
        ('matched_window', 'matched_thread', 'unmatched', 'routed', 'failed', 'last_receipt', 'last_source')]

class InputReceipts(c.Structure):
    _fields_ = [('struct_size', U32), ('version', U32), ('session_id', U64),
                ('accepted', U64), ('rejected', U64), ('duplicates', U64),
                ('input_kinds', U64 * 3), ('window_messages', U64), ('ping_candidates', U64)]

class Settings(c.Structure):
    _fields_ = [(n, U32) for n in ('struct_size', 'abi_version', 'sr_enabled', 'sr_backend', 'dlss_preset',
        'fg_enabled', 'fg_backend', 'generated_frames', 'latency', 'frame_limit_us', 'status_overlay', 'sr_quality', 'sharpening_mode')]
    _fields_ += [('sharpening_sharpness', F32), ('sharpening_mas', U32), ('sharpening_motion_sharpness', F32),
        ('sharpening_motion_threshold', F32), ('sharpening_motion_scale_limit', F32), ('sharpening_contrast_enabled', U32),
        ('sharpening_contrast', F32), ('sharpening_depth_scale', F32), ('sharpening_depth_bias', F32),
        ('sharpening_clamp_output', U32), ('sharpening_debug', U32)]

class RenderStatus(c.Structure):
    _fields_ = [(n, U32) for n in ('struct_size', 'abi_version', 'sr_enabled', 'sr_requested_backend', 'sr_effective_backend',
        'render_width', 'render_height', 'output_width', 'output_height', 'fg_requested_backend', 'fg_effective_backend',
        'fg_requested', 'fg_active', 'requested_generated', 'effective_generated', 'max_generated', 'foreground',
        'latency_requested', 'latency_effective', 'renderer_reason', 'fg_reason')]
    _fields_ += [(n, I32) for n in ('sr_result', 'fg_result', 'fg_vendor_result')]
    _fields_ += [(n, U32) for n in ('output_fps_valid', 'status_overlay', 'sr_backend_choices', 'fg_backend_choices')]
    _fields_ += [('base_fps', F32), ('output_fps', F32)]
    _fields_ += [(n, U64) for n in ('native_frame', 'source_frame', 'tagged_frames', 'application_presents', 'output_presents')]
    _fields_ += [('reason', c.c_char * 160)]

class CpuCounters(c.Structure):
    _fields_ = [('struct_size', U32), ('frames', U64), ('input_events', U64 * 3)]
    _fields_ += [(n, U64) for n in ('pings', 'substitutes', 'duplicates', 'order_violations', 'expired_messages',
        'unframed_inputs', 'derived_frame_ends')]

class LatencyStatus(c.Structure):
    _fields_ = [(n, U32) for n in ('struct_size', 'version', 'owner', 'available', 'effective_mode', 'pcl_available', 'report_available')]
    _fields_ += [(n, U64) for n in ('paced_frames', 'marker_calls', 'derived_boundaries')]
    _fields_ += [('events', CpuCounters)]

class LatencyFrame(c.Structure):
    _fields_ = [('struct_size', U32), ('version', U32), ('source', U64), ('generation', U64), ('ready_qpc', U64),
        ('paced', U32), ('failed', U32), ('completed', U32)]

class FgOptions(c.Structure):
    _fields_ = [(n, U32) for n in ('struct_size', 'mode', 'generated_frames', 'reflex_mode', 'frame_limit_us', 'debug')]

class VendorStatus(c.Structure):
    _fields_ = [(n, U32) for n in ('struct_size', 'supported', 'active', 'max_generated_frames', 'dynamic_supported', 'vsync_supported',
        'min_dimension', 'effective_generated_frames', 'effective_mode', 'pacing_owner', 'effective_reflex', 'low_latency_available', 'valid_statistics')]
    _fields_ += [(n, U64) for n in ('total_presented', 'generated_presented', 'version_id')]
    _fields_ += [('vendor_status', I32), ('version_name', c.c_char * 128), ('auto_supported', U32), ('configured_mode', U32),
        ('present_callbacks', U64), ('generated_callbacks', U64), ('mfg_unlock_status', U32), ('latency_paced_frames', U64)]

class SourceStatus(c.Structure):
    _fields_ = [('struct_size', U32), ('requested', FgOptions), ('vendor', VendorStatus), ('available', U32), ('reason', U32),
        ('last_result', I32), ('source_frame_id', U64), ('tagged_frames', U64), ('source_provenance', U32),
        ('input_qpc', U64), ('source_qpc', U64), ('source_qpc_frequency', U64)]

# Windows x64 C ABI, including padding before nested uint64 counters.
assert c.sizeof(Settings) == 96 and Settings.sr_quality.offset == 44 and Settings.sharpening_depth_bias.offset == 84
assert c.sizeof(RenderStatus) == 320 and RenderStatus.native_frame.offset == 120 and RenderStatus.reason.offset == 160
assert c.sizeof(CpuCounters) == 96 and CpuCounters.frames.offset == 8 and CpuCounters.input_events.offset == 16
assert c.sizeof(LatencyStatus) == 152 and LatencyStatus.paced_frames.offset == 32 and LatencyStatus.events.offset == 56
assert c.sizeof(LatencyFrame) == 48 and LatencyFrame.source.offset == 8 and LatencyFrame.paced.offset == 32
assert c.sizeof(FgOptions) == 24 and c.sizeof(VendorStatus) == 256 and VendorStatus.total_presented.offset == 56
assert VendorStatus.present_callbacks.offset == 224 and VendorStatus.latency_paced_frames.offset == 248
assert c.sizeof(SourceStatus) == 352 and SourceStatus.vendor.offset == 32 and SourceStatus.source_frame_id.offset == 304
assert SourceStatus.source_provenance.offset == 320 and SourceStatus.input_qpc.offset == 328

# Values from sr_session.h, backend.h and frame_generation.h. Advertised masks
# and the core settings API remain authoritative for this particular host/device.
PATCH_RANGES = {'sr_enabled': (0, 1), 'sr_backend': (0, 7), 'sr_quality': (0, 5),
    'fg_enabled': (0, 1), 'generated_frames': (1, 0xffffffff), 'latency': (0, 2)}
FG_BACKENDS = frozenset((1, 3, 4, 5, 7))
ACTIONS = ('input-routes', 'receipts-status', 'capture-sequence', 'capture-failures-on', 'capture-failures-off', 'capture', 'on', 'off', 'status', 'quality0', 'apply', 'latency-status', 'source-status')

class Refused(RuntimeError):
    pass

def plain(value):
    if isinstance(value, c.Structure):
        return {name: plain(getattr(value, name)) for name, _ in value._fields_}
    if isinstance(value, c.Array):
        return [plain(item) for item in value]
    if isinstance(value, bytes):
        return value.split(b'\0', 1)[0].decode('utf8', errors='replace')
    return value

def packet(kind, version=None):
    value = kind()  # Zero all bytes, including alignment padding and nested state.
    value.struct_size = c.sizeof(kind)
    if version is not None:
        setattr(value, 'abi_version' if hasattr(value, 'abi_version') else 'version', version)
    return value

def checked_packet(kind, raw, version=None):
    if len(raw) != c.sizeof(kind):
        raise Refused('Short public status/settings packet')
    value = kind.from_buffer_copy(raw)
    if value.struct_size != c.sizeof(kind):
        raise Refused('Unexpected public structure size')
    if version is not None and getattr(value, 'abi_version' if hasattr(value, 'abi_version') else 'version') != version:
        raise Refused('Unexpected public structure version')
    return value

def validate_patch(patch):
    if type(patch) is not dict or not patch:
        raise Refused('Patch must be a nonempty JSON object')
    normalized = {}
    for name, value in patch.items():
        if name not in PATCH_RANGES and name != 'fg_backend':
            raise Refused(f'Unknown or forbidden patch field: {name}')
        if name in ('sr_enabled', 'fg_enabled') and type(value) is bool:
            value = int(value)
        if type(value) is not int:
            raise Refused(f'{name} must be an integer (booleans only for enabled fields)')
        if name == 'fg_backend':
            if value not in FG_BACKENDS:
                raise Refused('Unsupported public FG backend enum')
        elif not PATCH_RANGES[name][0] <= value <= PATCH_RANGES[name][1]:
            raise Refused(f'{name} outside public range')
        normalized[name] = value
    return normalized

def load_patch(path):
    def unique(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise Refused(f'Duplicate patch field: {key}')
            result[key] = value
        return result
    data = Path(path).read_bytes()
    if len(data) > 4096:
        raise Refused('Patch exceeds bounded 4096 bytes')
    return validate_patch(json.loads(data, object_pairs_hook=unique,
        parse_constant=lambda value: (_ for _ in ()).throw(Refused(f'Nonfinite JSON value: {value}'))))

def patch_settings(original, patch, status):
    value = checked_packet(Settings, original, 3)
    patch = validate_patch(patch)
    for name, new_value in patch.items():
        setattr(value, name, new_value)
    for key, mask, enabled in (('sr_backend', status.sr_backend_choices, value.sr_enabled),
        ('fg_backend', status.fg_backend_choices, value.fg_enabled)):
        if key in patch or (enabled and key.replace('backend', 'enabled') in patch):
            if not mask & (1 << getattr(value, key)):
                raise Refused(f'{key} not advertised by current Runtime status')
    if 'generated_frames' in patch and status.max_generated and value.generated_frames > status.max_generated:
        raise Refused('Requested generated count exceeds advertised provider maximum')
    return bytes(value)

def apply_patch(api, patch):
    before = api.settings()
    wanted = patch_settings(before, patch, api.status())
    api.set_settings(wanted)  # RSF_OK=0, unlike native observation getters.
    after = api.settings()
    if after != wanted:
        raise Refused('Settings roundtrip differs; request may already be stored, no automatic rollback')
    status = plain(api.status())
    results = {key: status[key] for key in ('sr_result', 'fg_result', 'fg_vendor_result')}
    report = {'settings_roundtrip_verified': True, 'provider_acceptance_verified': False,
        'active_verified': False, 'settings': plain(checked_packet(Settings, after, 3)),
        'current_status': status, 'current_results': results,
        'note': 'Settings acceptance is a request, not provider activity or game/latency acceptance.'}
    failures = []
    if checked_packet(Settings, after, 3).sr_enabled and status['sr_result'] < 0:
        failures.append('SR currently reports refusal or deferred evaluation')
    if checked_packet(Settings, after, 3).fg_enabled and (status['fg_result'] < 0 or status['fg_vendor_result'] < 0):
        failures.append('FG currently reports refusal or deferred evaluation')
    report['runtime_refusals_or_deferrals'] = failures
    return report

class Image:
    def __init__(self, path, expected_hash):
        self.path = Path(path)
        self.data = self.path.read_bytes()
        if hashlib.sha256(self.data).hexdigest().lower() != expected_hash.lower():
            raise Refused('Runtime hash mismatch')
        self._parse()
    def _parse(self):
        data = self.data
        u16 = lambda pos: struct.unpack_from('<H', data, pos)[0]
        u32 = lambda pos: struct.unpack_from('<I', data, pos)[0]
        pe = u32(0x3c)
        if data[:2] != b'MZ' or data[pe:pe+4] != b'PE\0\0' or u16(pe+24) != 0x20b or u16(pe+4) != 0x8664:
            raise Refused('Expected x64 PE Runtime')
        self.size = u32(pe+24+56)
        section_table = pe+24+u16(pe+20)
        self.sections = []
        for index in range(u16(pe+6)):
            pos = section_table+40*index
            self.sections.append((u32(pos+12), max(u32(pos+8), u32(pos+16)), u32(pos+20), u32(pos+36)))
        def offset(rva):
            for va, size, raw, _ in self.sections:
                if va <= rva < va+size and raw+rva-va < len(data):
                    return raw+rva-va
            raise Refused('PE RVA outside file sections')
        export_rva, export_size = u32(pe+24+112), u32(pe+24+116)
        exports = offset(export_rva)
        functions, names, ordinals = [offset(u32(exports+i)) for i in (28, 32, 36)]
        self.exports = {}
        for index in range(u32(exports+24)):
            name_pos = offset(u32(names+4*index))
            name = data[name_pos:data.index(b'\0', name_pos)]
            rva = u32(functions+4*u16(ordinals+2*index))
            if export_rva <= rva < export_rva+export_size:
                continue  # A forwarded export is not a callable selected-module body.
            if self.executable(rva):
                self.exports[name.decode('ascii')] = rva
    def executable(self, rva):
        return 0 <= rva < self.size and any(va <= rva < va+size and flags & 0x20000000 for va, size, _, flags in self.sections)

class Remote:
    def __init__(self, pid, base, image):
        self.pid, self.base, self.image = pid, base, image
        self.process = self.memory = None
        self.running = False
        self.allowed = set()
        self.k = k = c.WinDLL('kernel32', use_last_error=True)
        k.OpenProcess.argtypes = [U32, c.c_int, U32]; k.OpenProcess.restype = c.c_void_p
        k.CreateRemoteThread.argtypes = [c.c_void_p, c.c_void_p, c.c_size_t, c.c_void_p, c.c_void_p, U32, c.c_void_p]
        k.CreateRemoteThread.restype = c.c_void_p
        k.WaitForSingleObject.argtypes = [c.c_void_p, U32]
        k.GetExitCodeThread.argtypes = [c.c_void_p, c.POINTER(U32)]
        k.CloseHandle.argtypes = [c.c_void_p]
        k.VirtualAllocEx.argtypes = [c.c_void_p, c.c_void_p, c.c_size_t, U32, U32]; k.VirtualAllocEx.restype = c.c_void_p
        k.VirtualFreeEx.argtypes = [c.c_void_p, c.c_void_p, c.c_size_t, U32]
        k.WriteProcessMemory.argtypes = [c.c_void_p, c.c_void_p, c.c_void_p, c.c_size_t, c.POINTER(c.c_size_t)]
        k.ReadProcessMemory.argtypes = [c.c_void_p, c.c_void_p, c.c_void_p, c.c_size_t, c.POINTER(c.c_size_t)]
        try:
            self.process = k.OpenProcess(0x43a, False, pid)
            if not self.process:
                raise c.WinError(c.get_last_error())
            psapi = c.WinDLL('psapi', use_last_error=True)
            psapi.GetModuleFileNameExW.argtypes = [c.c_void_p, c.c_void_p, c.c_wchar_p, U32]
            path = c.create_unicode_buffer(32768)
            if not psapi.GetModuleFileNameExW(self.process, base, path, len(path)):
                raise c.WinError(c.get_last_error())
            if Path(path.value).resolve() != image.path.resolve():
                raise Refused('Selected module path mismatch')
            self.memory = k.VirtualAllocEx(self.process, None, 4096, 0x3000, 4)
            if not self.memory:
                raise c.WinError(c.get_last_error())
            self.allow_export('rsf_get_overlay_api')
            self.write(0, struct.pack('<II', 88, 4)+bytes(80)); self._call(self.export('rsf_get_overlay_api'), self.memory)
            table = self.read(0, 88)
            if struct.unpack_from('<II', table) != (88, 4):
                raise Refused('Unexpected overlay API ABI')
            self.api = struct.unpack_from('<10Q', table, 8)
            for index in (7, 8, 9):
                if not image.executable(self.api[index]-base):
                    raise Refused('Overlay API pointer outside selected executable Runtime section')
                self.allowed.add(self.api[index])
        except BaseException:
            self.close()
            raise
    def export(self, name):
        if name not in self.image.exports:
            raise Refused(f'Missing, forwarded or non-executable public export: {name}')
        return self.base+self.image.exports[name]
    def allow_export(self, name):
        if name not in ('rsf_native_input_routes_get', 'rsf_native_input_receipts_get', 'rsf_d3d11_host_capture_control', 'rsf_get_overlay_api', 'rsf_d3d11_host_request_capture', 'rsf_native_latency_status_get',
            'rsf_native_fg_status_get', 'rsf_native_latency_frame_get'):
            raise Refused('Export is outside this bounded helper contract')
        address = self.export(name); self.allowed.add(address)
        return address
    def _call(self, address, argument, accepted=(0,)):
        if address not in self.allowed or not self.image.executable(address-self.base):
            raise Refused('Call is not an admitted public one-pointer API')
        thread = self.k.CreateRemoteThread(self.process, None, 0, address, argument, 0, None)
        if not thread:
            raise c.WinError(c.get_last_error())
        self.running = True
        try:
            if self.k.WaitForSingleObject(thread, 5000) != 0:
                raise Refused('Runtime action wait failed/timed out; remote allocation retained until process exit')
            self.running = False
            result = U32()
            if not self.k.GetExitCodeThread(thread, c.byref(result)):
                raise c.WinError(c.get_last_error())
            if result.value not in accepted:
                raise Refused(f'Public callback refused {I32(result.value).value} ({result.value:#x}); expected {accepted}')
            return result.value
        finally:
            self.k.CloseHandle(thread)
    def write(self, offset, value):
        if offset < 0 or offset+len(value) > 4096:
            raise Refused('Write exceeds bounded API scratch')
        copied = c.c_size_t()
        if not self.k.WriteProcessMemory(self.process, self.memory+offset, value, len(value), c.byref(copied)) or copied.value != len(value):
            raise c.WinError(c.get_last_error())
    def read(self, offset, size):
        if offset < 0 or offset+size > 4096:
            raise Refused('Read exceeds bounded API scratch')
        result = c.create_string_buffer(size); copied = c.c_size_t()
        if not self.k.ReadProcessMemory(self.process, self.memory+offset, result, size, c.byref(copied)) or copied.value != size:
            raise c.WinError(c.get_last_error())
        return result.raw
    def query(self, address, kind, version=None, accepted=(0,)):
        self.write(512, bytes(packet(kind, version)))
        self._call(address, self.memory+512, accepted)
        return checked_packet(kind, self.read(512, c.sizeof(kind)), version)
    def settings(self):
        return bytes(self.query(self.api[8], Settings, 3))
    def set_settings(self, value):
        checked_packet(Settings, value, 3)
        self.write(128, value); self._call(self.api[9], self.memory+128)
    def status(self):
        return self.query(self.api[7], RenderStatus, 3)
    def native_status(self, name, kind, version=None):
        return self.query(self.allow_export(name), kind, version, (1,))
    def source_status(self):
        value = self.native_status('rsf_native_fg_status_get', SourceStatus)
        if value.vendor.struct_size not in (0, c.sizeof(VendorStatus)) or value.requested.struct_size not in (0, c.sizeof(FgOptions)):
            raise Refused('Unexpected nested FG status/options size')
        report = plain(value)
        report['input_timestamp_known'] = bool(value.input_qpc)
        report['source_timestamp_known'] = bool(value.source_qpc and value.source_qpc_frequency)
        report['frame'] = {'known': False, 'source': value.source_frame_id}
        if value.source_frame_id and 'rsf_native_latency_frame_get' in self.image.exports:
            frame = packet(LatencyFrame, 1); frame.source = value.source_frame_id
            self.write(512, bytes(frame))
            result = self._call(self.allow_export('rsf_native_latency_frame_get'), self.memory+512, (0, 1))
            report['frame']['query_result'] = result
            if result == 1:
                frame = checked_packet(LatencyFrame, self.read(512, c.sizeof(frame)), 1)
                if frame.source != value.source_frame_id:
                    raise Refused('Latency frame lookup returned a different source')
                report['frame'] = dict(plain(frame), known=True, query_result=1)
        return report
    def close(self):
        if self.memory and not self.running:
            self.k.VirtualFreeEx(self.process, self.memory, 0, 0x8000)
        if self.process:
            self.k.CloseHandle(self.process)
        self.process = self.memory = None

def main(argv=None):
    argv = sys.argv[1:] if argv is None else argv
    if len(argv) not in (5, 6):
        raise Refused('Usage: PID MODULE_BASE RUNTIME_DLL ACTION SHA256 [PATCH_JSON for apply]')
    pid, base, dll, action, sha = argv[:5]
    pid, base = int(pid), int(base)
    if pid <= 0 or base <= 0 or action not in ACTIONS or (action == 'apply') != (len(argv) == 6):
        raise Refused('Invalid caller-owned PID/module/action/patch arguments')
    patch = load_patch(argv[5]) if action == 'apply' else None
    image = Image(dll, sha)
    remote = Remote(pid, base, image)
    try:
        if action == 'status':
            print(json.dumps(dict(plain(remote.status()), pid=pid), indent=2)); return 0
        if action == 'input-routes':
            print(json.dumps(dict(plain(remote.native_status('rsf_native_input_routes_get', InputRoutes, 1)), pid=pid), indent=2)); return 0
        if action == 'receipts-status':
            print(json.dumps(dict(plain(remote.native_status('rsf_native_input_receipts_get', InputReceipts, 1)), pid=pid), indent=2)); return 0
        if action == 'latency-status':
            value = remote.native_status('rsf_native_latency_status_get', LatencyStatus, 1)
            if value.events.struct_size not in (0, c.sizeof(CpuCounters)):
                raise Refused('Unexpected nested CPU counter size')
            print(json.dumps(dict(plain(value), pid=pid), indent=2)); return 0
        if action == 'source-status':
            print(json.dumps(dict(remote.source_status(), pid=pid), indent=2)); return 0
        if action == 'apply':
            report = apply_patch(remote, patch); report['pid'] = pid
            print(json.dumps(report, indent=2))
            return 2 if report['runtime_refusals_or_deferrals'] else 0
        if action in ('capture-sequence', 'capture-failures-on', 'capture-failures-off'):
            operation = 1 if action == 'capture-sequence' else 2
            value = int(action != 'capture-failures-off')
            remote.write(512, struct.pack('<4I', 16, 1, operation, value))
            remote._call(remote.allow_export('rsf_d3d11_host_capture_control'), remote.memory+512)
            print(json.dumps({'pid': pid, 'action': action, 'accepted': True, 'capture_completed': False}))
            return 0
        if action == 'capture':
            remote._call(remote.allow_export('rsf_d3d11_host_request_capture'), None)
        else:
            settings = bytearray(remote.settings())
            if struct.unpack_from('<I', settings, 20)[0] or struct.unpack_from('<I', settings, 32)[0]:
                raise Refused('Native-only action refused: FG or latency enabled')
            struct.pack_into('<I', settings, 44 if action == 'quality0' else 8, 0 if action == 'quality0' else int(action == 'on'))
            remote.set_settings(bytes(settings))
            if remote.settings() != bytes(settings):
                raise Refused('Legacy action roundtrip differs; no automatic rollback')
            print('overlay settings SR enabled/backend:', struct.unpack_from('<II', settings, 8))
            print('overlay quality:', struct.unpack_from('<I', settings, 44)[0])
            print('overlay FG enabled/backend/generated:', struct.unpack_from('<3I', settings, 20))
            print('overlay latency:', struct.unpack_from('<I', settings, 32)[0])
        print(f'RSF {action} action PID={pid} result=0')
        return 0
    finally:
        remote.close()

if __name__ == '__main__':
    try:
        sys.exit(main())
    except Exception as error:
        print(json.dumps({'error': str(error), 'provider_acceptance_verified': False}), file=sys.stderr)
        sys.exit(2)

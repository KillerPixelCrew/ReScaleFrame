# Prospective CPU source refusal, 15 November 2026

Question: can a real simulation begin following a refused prospective input bind
produce latency markers or an input timestamp inserted after simulation began?
This source review follows A8, B1 and E3 in the local Opus review at
`.local/skyrim-research/opus-review-20261115.md`. No build, device fixture, game,
deployment or commit was run for these changes.

The adapter's void fact callback cannot acknowledge the core BIND verdict. The
core therefore keeps actual simulation begin/end timestamps for an adopted or
aborted reservation whose bind was refused, without synthesizing INPUT_SAMPLE.
Its stage masks may be 13 before closure and 29 afterward: the absent input bit
is deliberate. Simulation facts do not reach the latency sink in this case;
frame begin/end still do, so ownership and retirement remain intact. The marker
entry point also refuses reservations other than BOUND/CLOSED. Valid bound polls
still promote their actual poll-entry timestamp, and measured and rendered-only
clients retain their existing paths. No camera/frame or callback ABI changed.

B1 is safe refusal, not a reason to infer consumption on excluded frames. The
measured FG admission in `native_fg.cpp` requires real input/simulation bits,
no substitute input, an eligible pacing state and, for a prospective owner, a
BOUND/CLOSED reservation with the actual poll after readiness. An unbound source
fails this gate; partial closure aborts its provider frame. Frames without a
prospective record retain the existing explicitly labelled substitute path and
cannot pass measured FG admission. There is no late sleep or per-frame switch
to rendered-source provenance to preserve FG during free camera/load/withdrawal.
A sleep already completed at RESERVE cannot be undone when later binding fails;
it remains a real pre-poll sleep, not a successful input-consumption claim.
Thus excluded frames can lose FG; this is an acceptance limitation, not missing
input to fabricate. Per-frame provider fallback is an optional design proposal.

E3's full owner sweep now runs at SOURCE_RESERVE, SOURCE_ABORT, FRAME_BEGIN and
FRAME_END only. RESERVE can replace/overlap live polls, BEGIN adopts/withdraws
them, ABORT withdraws explicitly, and END closes or marks unbound. SAMPLED and
BIND do not invalidate another owner; input/window/simulation facts do not alter
reservation lifetime. Recursive derived frame closure still delivers FRAME_END.
A rejected RESERVE has never reached the sink; its withdrawals are observed at
the next delivered lifetime boundary, as before. Present outcomes affect pacing
dependencies, not reservation lifetime. Explicit session end still clears the
CPU ownership and reservation records through the existing retirement path.

Focused source changes add reservation tests for aborted and adopted/refused
binds, retained real simulation timestamps, absent fabricated input, no latency
callback, marker refusal and clean closure. The existing FG session fixture also
checks that an aborted frame cannot prepare or emit vendor markers. These are
added tests awaiting coordinator execution, not reported passing runtime tests.

## R8G8 diagnostic readback correction

The coordinator's matched-carrier PID44520 trial at
`.local/skyrim-research/parity-acceptance-20261115-233223` exited normally and
restored four deployed files. Its capture
`scene-20261115-233252-100-43887781-44520-1` was nevertheless partial:
`native_temporal_mask` raw/preview returned -3 (unsupported format), with the
actual descriptor 1920x1080 DXGI49 R8G8_UNORM. The R32 reactive/bias inputs at
1280x720 were captured. Controller `observation_completed` was false; process
exit alone did not establish capture completion.

The rejection is in the generic dumpers: `texture_pixel_bytes` lacked format49,
so raw and preview paths refused before decoding/readback; `decode` also lacked
its numeric two-channel interpretation. Both now support R8G8_UNORM with packed
two-byte raw pixels and independent R/G values, including valid zero samples.
The existing texture dump fixture adds byte-exact raw and numeric preview checks.
This does not change SR mask processing or establish pickup ghosting's cause or
repair. A further F10 is still needed to compare actual raw 0.1R+G values against
the normalized mask; no such comparison was performed here.

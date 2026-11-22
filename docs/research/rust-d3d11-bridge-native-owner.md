# Rust D3D11 bridge state and linked device checks

Base `9716068b`. This continuation traces the production factory hooks,
facade descriptor and COM identities, Streamline graphics, command slots and
the two linked device fixtures. It reuses the typed DXGI factory, device and
GPU helpers already in the base and does not edit the shared D3D11 host.

The old WRL capsule owned the parent factory, source device and context,
direct D3D12 device and queue, and the upload fence. The Rust capsule keeps
the same references. The adapter is a temporary initialization input.
Description reads take a snapshot independently of GPU work. Source queue
acquisition takes one reference after any required proxy unwrap. The
provider list stays borrowed through its slot lease. A failed drain or
checked teardown still retains GPU-reachable references. Successful
retirement drops the direct objects and the private host in the established
owner order.

The C++ shim retains factory detours and their `STDMETHODCALLTYPE` signatures,
MinHook install and rollback, the guarded original factory call and the
native SDK entry points. No public C name or private platform slot moved.
The typed native-wrap route requires the same source device, parent factory
and owner check. The dispatch entry points now route through Rust's private
continuations, using the existing exact-receiver admission and Release
behavior. The explicit HWND creation path used by D08 retains the supplied
child window and queue. D08's parent input window and child DXGI HWND are
still distinct, with no changes to its caller or window ownership.

The D3D11 fixture now assembles and evaluates the initial shared SR frame on
the existing Streamline host from Rust, retaining all nine source and hint
textures in the fixture. It checks the same red half-float readback. The
queued scene, submission, Slate window and sampled binding packet creation
now uses Rust alongside the linked AC7 scope callbacks. The native runner
still owns the enabled frame loop, CPU/Present and SDK assertions, device
entry and final teardown. Keep its PORT fixture entry open.

The FG fixture now owns source command reset, input callback, transition to
RENDER_TARGET, submission, and the joined render-window callback in Rust.
It invokes the production overlay in sequence before the finish command
transitions the same backbuffer to PRESENT. Rust owns the opt-in foreground
lease and watchdog, plus the typed D3D12 validation-message scan used by
both fixtures. The native runner still owns vendor callbacks, mode setup,
status and switch assertions, and the final Present/drain loop. Keep its
PORT fixture entry open. Adapter selection, hardware/WARP smoke, CTest names,
skip codes and production linkage still follow the existing fixture routes.

Source checks compared 33 public C names, all 72 private platform slots,
native hook bodies, explicit HWND forwarding, retirement branches, all
owned dispatch methods and both fixture command sequences against the base.
Rust 1.94.1 formatting, repeatable targeted bindgen 0.72.1 generation,
locked offline Windows-target Cargo metadata and Git whitespace checks
passed. Builds, executable fixtures, lint and device runs are pending.



## Rust factory install transaction

From `c34634bc`, the Rust factory owner calls setup admission, the real
Factory2 bootstrap, MinHook initialization, legacy and HWND hook creation,
and both enables in that order. The two original addresses are published
before the first enable. Until both enables complete, an entered detour
forwards to its own native original. Scoped setup and successful publication
share the same install transaction. The Factory2 reference remains live
through installation and the success log.

A refused second creation removes the first hook. A refused enable retains
the legacy disable and both remove attempts. A clean failure clears the
originals and permits another cold attempt. A refused removal blocks
reinstallation and retains any published continuation whose hook may
still be registered for native replay. The pinned MinHook implementation only reports successful removal
after freeing the trampoline and deleting the hook entry. Its absent entry
and absent heap results also permit clearing the original. This remains a
cold setup transaction, with no live uninstall path.

The native file still executes Factory2 creation, individual MinHook calls,
the typed legacy and HWND original replays, and the C++ exception guards.
The SDK and C linkage anchors retain their existing contracts. The install
leaf table is separate from the 72-slot presentation platform, so no fake
fixture slots moved. D08 explicit HWND creation, O02 and shared SR dispatch
retain their existing routes. Source, syntax and binding evidence does not
establish live hook or device behavior.

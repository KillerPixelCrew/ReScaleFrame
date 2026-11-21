# Presentation synchronization ownership in Rust

The source comparison starts at `fab9fe5a`, which includes the plain physical swapchain plan at
`b36c9864` and [command-slot ownership](rust-d3d11-present-slots.md) at `dcfbf8cb`. The question was
whether the existing Rust presentation owner could take the interop queue, completion fence and
two event handles off the C++ swapchain facade while preserving the lifetime of the original
objects. The port follows the existing `ComRef` and leaf-closed handle patterns. The C++ facade
still implements `IDXGISwapChain4` and calls the same graphics and Win32 APIs.

## Creation and publication

1. `init_source`, `native_device`, Streamline host adoption or creation, and `resolve_graphics`
   still run before synchronization setup. On native D3D12 the interop lease takes one additional
   reference to the exact `direct_queue` interface queried from the engine. On D3D11 the owner
   requests a separate direct queue from `graphics.device`, which may be Streamline's proxy
   device. The native leaf zero-initializes the descriptor and sets only `Type = DIRECT`.
2. The completion event is unnamed, non-inheritable, auto-reset and initially nonsignaled.
   The completion fence comes next, also on `graphics.device`, at value zero with
   `D3D12_FENCE_FLAG_NONE`. Native D3D12 with runtime switching and the application's
   `FRAME_LATENCY_WAITABLE_OBJECT` flag then requests a second event with the same properties
   except that it starts signaled. `PresentationSync` keeps each acquired reference or handle
   as the stages run. The owner stores that partial state before returning any `E_FAIL` so final
   Release of the unpublished facade handles the rollback.
3. After successful initialization, the graphics snapshot publishes the same graphics device
   and the interop queue, with `native_queue` cleared. Unity's `create_shared` and the native FG/SR
   graphics callers still get the shared upload queue. `rsf_d3d11_present_source_queue` holds the
   owner guard and passes the leased queue to the same C++ unwrap and AddRef logic. Native D3D12
   returns the engine queue. D3D11 with a Streamline host unwraps the proxy queue. The acquired
   reference still belongs to the D3D12 FG input record in `fg12.rs`, which compares its identity
   when reusing textures and signals submission on that queue. The physical queue export still
   borrows the current provider's creation queue from `Physical`.

## Resource uses during presentation

| Use site | Source comparison |
| --- | --- |
| Native D3D12 `Present` | Rust compares the same queue interface pointers. If they differ, the native leaf signals from the engine queue and waits on the physical queue before slot reset. After a successful submission the signaller and waiter reverse before physical Present. A refused return join still leaves the submitted list covered by the slot signal. |
| D3D11 `Present` | Both paths still wait on the completion fence and event before resetting the allocator and list. D3D11 still submits the final upload and return synchronization through the physical queue using its separate shared D3D11/D3D12 fence. Both paths still signal the original completion fence on the physical queue at the slot boundary. |
| Application latency and resize | The Rust latency accessors check for the same optional event. Frame completion still signals it immediately when below the latency limit, or arms it on the completion fence at the selected value. Initialization and the existing application-frame reset on resize still signal it immediately. Provider replacement keeps the event. |
| `GetFrameLatencyWaitableObject` | Native D3D12 with switching asks the Rust owner for a duplicate. The C++ leaf uses the current process for both ends of `DuplicateHandle`, with `DUPLICATE_SAME_ACCESS` and inheritance disabled. The caller owns the duplicate. Native D3D12 without switching still queries the physical chain's waitable object, and the D3D11 path still returns null. |
| Drain for switch, resize and destruction | With a graphics queue and completion fence, the owner signals and CPU-waits on the interop queue first when present, then on the graphics queue using the next value. The two steps still run when those pointers alias. The provider's retirement fence is then waited on with the same completion event. The native wait adapter still uses the existing fence helper and its 10-second budget. |
| Lazy provider retirement fence | C++ still creates it lazily on `graphics.native_device`. `retire_submit` still waits on the vendor fence, submits the optional list, and signals the private retirement fence. Moving the explicit release point into a native leaf keeps its order relative to the Rust-owned completion fence. |

Native calls use borrowed pointers under the facade's lifetime or the existing owner guard.
Drain also borrows the graphics queue resolved at initialization, so it can read the queue
without consulting the mutable swapchain description. The Rust mutex protects snapshots and
moves of the leases. It is released before native calls.

## Rollback and final Release

Before the completion fence exists, drain skips the queues. A refusal creating the optional
ready event happens after the completion fence exists, so it still drains both queues before
cleanup. The failed-drain branch retains the queue, completion fence, both acquired event
handles, physical chain, buffers, slots and the remaining C++ GPU references. The original raw
event members also stayed open on this branch. The replacement refusal flag stays set.

Once drained, retirement still drops shared D3D11 buffers and their upload fence, the physical
chain, and the provider before closing the completion event and then the application-ready event.
Native D3D12 render buffers still follow the private host's teardown, if any.

- With a private host, Rust releases the interop reference after the events, then the slot pairs
  in slot order and then the completion fence. C++ releases `retired`, the context, D3D11 device
  and factory before destroying that host. The facade still releases its direct queue/device.
- With a shared host, Rust retains the interop reference for the process after closing the events.
  Without a host it holds that reference through the releases below.
- With a shared or absent host, Rust releases the slots last-slot first, then the facade's
  retirement fence through its leaf, the completion fence, and the facade's direct queue and
  device through their leaf. The no-host interop reference follows, before the other facade
  members end. This follows the former C++ reverse member destruction order.

## Private ABI and verification

`rsf_d3d11p_facts` no longer includes the interop pointer. Its x64 size is 112 bytes, with
`device11` at 96 and `buffer_usage` at 104. The private platform table has 73 entries. The
creation leaves transfer owned COM or event references into Rust. Graphics leaves borrow the
fence, handle and queues. The Rust and C++ mirrors assert the moved offsets, including slots
29–33 for synchronization creation and event effects, 44 for join, 59 for drain signal/wait,
67–69 for the split native releases, and 72 for the final field. The extra Rust facade call for
duplication is internal. The public bridge header and exported facade definitions match the base.

The adapted fake platform checks which device and queues the owner passes, the added reference
on native aliasing, readiness event selection and duplication, completion-fence use, and the
shared and private host release paths. The partial-initialization fixture covers refusals in
each creation stage, including a drain refusal after the optional event fails. The fake's
reset now clears `ACTIVE_GRAPHICS`, which replaced the stale `ACTIVE_INTEROP` name in the base.

The source comparison matched the names, order and signatures of all 73 table entries across
the C and Rust mirrors, and all 33 public facade signatures against the base. Pinned Rust
formatting, locked offline Cargo metadata and whitespace checks passed. The user owns compilation,
linking, Clippy, tests, device and game verification, and deployment. Device checks remain for
native D3D12 switching with a retained application wait handle, the D3D11 interop/physical queue
split, and teardown with shared and private hosts.

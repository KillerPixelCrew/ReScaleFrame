# Graphics

Resource handling, API interoperability and presentation belong here. What exists so far is the
observation side: reading what a game renders, so an integration can be built against measured
facts rather than assumptions.

| Unit | Purpose |
| --- | --- |
| `texture_dump` | Copy a D3D11 texture off the GPU and write it somewhere it can be looked at |
| `d3d11_observer` | Watch a process's D3D11 use without altering what it renders |

## Texture dump

Copies through a staging resource, so the game's own resource and binding state are untouched.
Writes a Targa and a JSON of the decoded ranges.

The velocity view implements Unreal's encoding as its source states it,
`In * (0.499 * 0.5) + 32767/65535`, rather than a value fitted to captured data. Decoding is
`(value - 32767/65535) / 0.2495`. Raw zero is the clear value meaning nothing wrote velocity at
that pixel, so it is marked rather than decoded, and zero motion sits at mid grey so sign reads at
a glance. The marker is blue: it shared the red channel with the horizontal motion component once,
and absent motion read as strong motion for several sessions.

Unsupported formats are refused rather than decoded as something they are not. A wrong decode
looks plausible, which makes it worse than no answer.

## Observer

Patches two shared vtable entries, `ID3D11Device::CreateTexture2D` and `IDXGISwapChain::Present`,
using slot indices from the table `tools/ghidra/build-directx-types.py` generates. Both forward to
the original. It changes no resource, no binding and no draw.

Creation is watched rather than binding. Binding runs about 125 times a frame in AC7 and carries
nothing creation does not. Targets are matched by signature, format and size, because resource
identifiers from a frame capture do not exist at runtime.

Constant buffers are matched by a size range and every size seen is counted. Unreal's D3D11
buffers land in power of two pool buckets, so a 2640 byte view structure is allocated as 4096 and
matching the stock size exactly found nothing at all. When a guess misses, the histogram is what
identifies the right buffer on the next run.

Dumps are requested from any thread and carried out inside the next present. A device context
cannot be used from two threads at once: reading from a worker races the game's rendering, returns
whatever the staging copy happened to hold, and can take the process down. Present is the one
moment we are already on the rendering thread at a defined point in the frame.

Nothing calls D3D11 while holding the observer's lock. These hooks run inside the runtime's own
code, so any call made from one can re-enter it, and re-entering a non-recursive lock deadlocks
the process. State is recorded under the lock and the work is done outside it, against a snapshot.

## Tests

`texture_dump` builds a texture with four known pixels and checks the decoded extremes against
values computed independently in the test. `d3d11_observer` installs, then behaves like a game:
one matching target and three that must be ignored for three different reasons, a constant buffer
of the watched size and one that is not, a dump carried out inside a present, and a present after
uninstalling to prove the vtable entries were restored intact.

Both run under Wine on DXVK, which is the same D3D11 the game sees on a Linux development
machine, and skip cleanly where no device can be created. Both carry a timeout, because a deadlock
is a realistic failure for this code and should fail in two minutes rather than hang the suite.

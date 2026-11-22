# Backends

| Component | Purpose | Status |
| --- | --- | --- |
| `dlss` | Public C headers (`rescaleframe/dlss.h`); the Streamline adapter is in the Rust orchestrator | D3D11/D3D12 SR, DLSS-G and Reflex/PCL implemented; recorded live AC7/Unity paths |
| `fsr` | FFX DX12 SR provider with explicit FSR2/FSR3/FSR4 version selection | FSR2/3/4 SR device evidence including guarded FSR4 INT8; FSR3/4 FG implemented with 2x maximum, exact hardware support queried |
| `xess` | XeSS-SR DX12 provider | DX12 SR and XeSS FG/XeLL implemented; native D3D11 SR unavailable; queried MFG limits |

Without SDK headers, providers return `NOT_COMPILED`. With headers, `open` loads the runtime
from an absolute configured directory, resolves entry points, creates the vendor context and checks
support. Probe reports build capability without loading. Missing DLLs, unsupported versions and
failed evaluations remain different outcomes. Current AMD SDK 2.3.0 supplies actual FSR 2.3.4,
3.1.5 and hardware-dependent FSR4 providers; the family is never silently substituted.

The orchestrator owns shared resources and frame sequencing. Each backend owns its vendor context, capability queries, input requirements, and evaluation. Streamline SDK calls are Rust in the orchestrator over the official headers' layouts (checked by fixtures). No frame generation SDK runs on D3D11; the implemented D3D12 presentation/interop services host those paths ([vendor contracts](../../docs/research/vendor-fg-contracts.md)).

Motion conventions, the Streamline adapter history and the Wine development notes are in [DLSS backend notes](../../docs/research/dlss-backend-notes.md). [Current status](../../docs/current-status.md) summarizes the limits.

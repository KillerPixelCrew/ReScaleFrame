# AC7 F9 before-draw rows in Rust

The comparison uses `b25f1ff1ef0550ce41a8da3efa5a196b812b90c4`, the
[geometry row baseline](rust-ac7-motion-geometry.md). The before-draw sources at worktree
HEAD `17904281d1bc851e31c98e74ebb9ada1bfbc823e` match it. This port moves the two remaining
native draw-stream writers and their observation plans into the existing AC7 Rust capture
owner. It uses the shader identities already observed for the [TrueSky composite](ac7-plane-artifacts-20261002.md#measured-truesky-composite-and-capture)
and [directional contact light](ac7-lighting-shadow-20261004.md#fresh-capture-isolates-the-directional-light-draw).

## Admission and effect order

The frame tap reports an observed-context draw before forwarding it, with the target ordinal
also used by the after callback. The capture callback accepts sampled Present ages 0, 30 and
59. An inactive sample or an allocation below 400 pixels wide or 200 pixels high exits before
shader lookup. The plan recognizes PS CRC32C `0x83524e47` for cloud and `3619939816` or
`4154163049` for contact light. An unrecognized hash skips the row. The first qualifying
contact-light draw claims its before/after pair only if the sampled interval is still current.
A later contact draw in the same interval skips the before row. The post-draw tap still consumes
the pending after observation only for those two hashes, after its ordinary row preflight.

| Row | Native effects in order |
| --- | --- |
| `cloud_composite_before` | Take QPC ticks and the thread ID. Describe and snapshot the tap's colour resource. Query MRT 0–1 and describe and snapshot MRT 1 when bound. Query live PS SRVs 0–3 and describe and snapshot the bound inputs in slot order as `cloud_near`, `cloud_far`, `cloud_near_far_depth`, `cloud_scene_depth`. Submit the copied row. |
| `contact_light_before` | Claim the pair, then take QPC ticks and the thread ID. Describe and snapshot the tap's colour resource. Query live PS SRVs 0–5 and describe and snapshot the bound inputs in slot order as `contact_ps_input_0`, `contact_ps_input_1`, `contact_ps_input_2`, `contact_light_mask`, `contact_scene_depth`, `contact_static_shadow`. Query PS samplers 0–5, describe and release each bound sampler in order, then query live PS CBs 0–1 and visit them with the lighting quota. Copy the queued scope and submit the row. |

Native code keeps the typed bindings and descriptors, scoped COM references, texture readback
and file effects, constant-buffer readback and blob deduplication, and the native RHI reads
used for queued-scope lookup. Rust supplies the dump roles and live PS query ranges. The row
contains copied facts and numeric identities. The ordinary draw stream still decides final
admission after the before-draw effects and applies the shared 12,000-row limit. A completed
row can retain its original sample age while that session is still active. Texture ordinals
and saved constant blobs can be consumed even when the row is dropped at the final check.

## Field and receipt comparison

| Row or nested value | Key order and conditions |
| --- | --- |
| Cloud | `kind`, `qpc`, `interval`, `thread`, `target`, `draw_index`, `ps_hash`, `colour`, `loss`, `inputs`. An unbound MRT 1 emits `loss: null`. |
| Contact light | `kind`, `qpc`, `interval`, `thread`, `ps_hash`, `colour`, `inputs`, `samplers`, `ps_cb`, `rhi_command`, `rhi_execute`, `queued_native_scope`, `queued_native_view`, `queued_native_family`, `queued_native_frame`, `queued_list`, `queued_uid`. |
| Colour, loss and PS inputs | The bound inputs start with `slot`, followed by `view`. Colour and loss start with `view`. The resource description starts with `resource`, `dimension`, and uses `width`, `height`, optional 3D `depth`, and `format` on the same dimensions as the native writer. The optional `snapshot`, `snapshot_result` keys follow the view. |
| Sampler | `slot`, `filter`, `address` in U/V/W order, `comparison`, `lod` in min/max order. Finite LODs retain default stream precision of six significant digits. |
| Constant buffer | `slot`, `buffer`, `blob`, `upload_serial`, `readback`. The lighting path uses live readback with zero upload serial. `readback: 1` indicates a reserved attempt, including an attempt that failed. Zero blob ID represents empty or dropped bytes. |

The native texture receipt still uses the basename `f<interval>_stage_<role>_<ordinal>`.
An unavailable sample, a resource that cannot be queried as a 2D texture, or an exhausted quota
before reservation leaves both receipt fields absent. A reserved dump returns the exact basename
and signed result even when readback or file writing fails. Cloud uses the shared 48-read
threshold and lighting can use slots up to the existing 72-read session cap. The row writer uses the
receipt from native rather than inferring success from budget, texture metadata or file name.

## Archived evidence and verification

The local AC7 archive contained 42 cloud and 18 contact-light before rows across 14 sessions.
Each preceded an ordinary draw matching interval, thread, and PS hash. All cloud pairs also
matched target and draw ordinal. Receipt ordinals increased in the serialized view order in all
60 rows. The cloud field order matched throughout. The six rows for the
original contact-light shader from `motion-20261004-082205-100100-1` and
`motion-20261004-082226-100100-2` predate the sampler fields and the full six-input plan. All 12
rows for the phased shader matched the baseline's ordered keys, including samplers and both
live CBs. All 72 archived sampler entries used the LOD text `[0,3.40282e+38]`.

In `motion-20261014-183755-40356-5`, cloud slot 3 at interval 59 retained its view with no
receipt after slot 2 used ordinal 47, consistent with the shared threshold. Across the archive,
all 317 emitted before-draw texture results were zero, and all cloud rows had a null loss MRT.
Present loss MRT and nonzero snapshot results were compared against source. The archive did
not exercise those branches or show which draws failed eligibility.

Source review, archived JSONL comparison, Rust 1.94.1 formatting, locked offline Windows Cargo
metadata and Git whitespace checks passed. Compile and link, Clippy, fixtures, fresh capture
comparison, device and game verification, deployment and push remain user-owned.

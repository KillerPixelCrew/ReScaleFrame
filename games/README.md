# Game plugins, and what is recorded about each game

One directory per game, holding its plugin and everything known about how it renders. Beside the
code, each keeps an `engine.json`: the facts that decide whether a reconstruction can be attached to
it at all, and how each one was established.

## Why this file exists

No public database records an engine's minor version. PCGamingWiki has 1727 Unreal Engine 4 games
and records the family for all of them and the version for none, so the question that actually
predicts a game's profile, which engine build and therefore which temporal machinery it has, cannot
be looked up anywhere. Surveying its antialiasing notes gets partway: of the 641 games that document
their antialiasing at all, 40 name only a non-temporal technique and were released while UE4's own
temporal antialiasing was at its worst. That is a shortlist, not an answer, and the remaining 1086
games document nothing.

So it gets written down here, one game at a time, as each is examined.

## What goes in `engine.json`

Every claim carries how it was established. The fields are small and the discipline is the point:

- `measured` means it was observed in the running game or read out of a capture.
- `inferred` means it follows from something measured, with the reasoning stated.
- `assumed` means it is a working hypothesis nothing has confirmed.

An `assumed` entry is not a defect. Writing `assumed` where the truth is unknown is the whole
purpose, because every wrong turn this project has taken began with an assumption recorded as a
fact. A field with nothing behind it is left out rather than guessed at.

## The fields that predict whether a game is workable

Grouped as they are asked, in the order of the search in
[the skill](../.claude/skills/game-render-analysis/SKILL.md):

| Field | Why it decides anything |
| --- | --- |
| `engine.version` | Which temporal machinery exists at all, and where its members sit in the view buffer |
| `velocity.present` | Whether a velocity buffer is written, whatever the game's settings claim |
| `velocity.encoding` | Whether a decode pass is needed before a backend can read it |
| `velocity.camera_motion_included` | Whether the backend can reconstruct camera motion or must be handed it |
| `jitter.state` | Absent, gated behind a condition, or already running |
| `temporal_pass.state` | What consumes those inputs today, and whether it can be replaced |
| `insertion_point` | Where in the frame the reconstruction goes, and what is not in the colour before it |

## Determining an engine version without asking the game

In order of how much they cost:

1. **Strings in the executable.** Unreal writes `++UE4+Release-4.18` and similar. Free when the
   binary is not encrypted, which it often is.
2. **The view uniform buffer's shape.** Members were added over time, so their presence dates the
   build. `ViewToClipNoAA` arrived after 4.18, and its absence is what placed Ace Combat 7 while
   also explaining why the stock layout stopped predicting the buffer partway through.
3. **Asset and package versions** in `.pak` and `.uasset` headers, which are versioned separately
   but move with the engine.

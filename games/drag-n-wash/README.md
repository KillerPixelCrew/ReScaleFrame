# Drag'n Wash

This is the third research case and the first Unity game. It is intended to validate the shared
[Unity Mono plugin](../unity-mono/README.md), rather than receive a duplicated game renderer DLL.

The examined Steam build is `25286774`, app `4739660`: Windows x64, Unity `6000.3.14f1`, Mono,
URP Forward+ and a RenderGraph implementation. D3D12 then D3D11 are packaged API candidates;
the active API has not been observed. No game launch, injection or installation change was made.

The shared native scaffold recognizes this exact executable fingerprint. It refuses preparation
because the Mono bootstrap, Harmony adapter and renderer hooks are not implemented. Its DLL
contract is synthetic-tested separately from the game.

[Engine evidence](engine.json) records identity and capability limits.
[Renderer research](../../docs/research/drag-n-wash-renderer.md) records the file/source findings,
temporal/UI boundaries, parser failures and pending live experiments. Rendering, SR, FG and
latency support remain unimplemented and untested for this game.

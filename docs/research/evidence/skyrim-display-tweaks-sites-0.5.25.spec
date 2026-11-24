# SSEDisplayTweaks.dll 0.5.25 (SHA-256 df7f352a...fdd8ba9f4) sites for tools/derive-site-patterns.py:
# the creation wrapper the relay jumps to, and its call of the retained game thunk through
# `mov r10,[rip+slot]` ... `call r10; mov ebx,eax`. games/skyrim-se/src/discovery.rs holds them.
code display_entry 27250 20
code display_invoke 272e9 77

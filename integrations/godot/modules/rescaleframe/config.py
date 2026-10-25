from pathlib import Path


def can_build(env, platform):
    return platform == "windows"


def configure(env):
    module = Path(__file__).resolve().parent
    root = module.parents[3]
    env.Append(CPPPATH=[str(module), str(root / "sdk/game/include"), str(root / "integrations/godot/native")])

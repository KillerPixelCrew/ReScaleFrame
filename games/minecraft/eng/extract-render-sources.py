"""Extract the mapped renderer sources used by this port; never touches the game."""
from pathlib import Path
import zipfile

root=Path(__file__).resolve().parents[1]
source=root/'build/neoForm/neoFormJoined1.21.1-20240808.144430/steps/patchUserDev/outputs.jar'
out=root/'artifacts/minecraft-sources'
prefixes=('net/minecraft/client/renderer/', 'net/minecraft/client/model/geom/',
          'net/minecraft/client/particle/', 'com/mojang/blaze3d/',
          'net/neoforged/neoforge/client/event/', 'net/minecraft/client/Minecraft.java')
with zipfile.ZipFile(source) as archive:
    count=0
    for name in archive.namelist():
        if name.endswith('.java') and name.startswith(prefixes):
            dest=out/name;dest.parent.mkdir(parents=True,exist_ok=True)
            dest.write_bytes(archive.read(name));count+=1
print(f'Extracted {count} mapped renderer sources to {out}')

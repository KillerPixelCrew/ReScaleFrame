"""Summarise the render targets and pass structure of a RenderDoc capture.

Takes the XML that `renderdoccmd convert -c xml` produces, which is structured data only and
needs no replay device. On Linux the conversion itself has to run through the Windows
renderdoccmd under Wine, because only the Windows build can parse D3D11 chunks.

This reports what the frame allocated and how it was bound. It does not read pixels, so it can
say a target is a two channel float at render resolution, but not what convention its contents
follow. That still needs looking at the image.
"""

import argparse
import json
from collections import Counter, defaultdict
from pathlib import Path
from xml.etree import ElementTree

# Formats that a velocity target plausibly uses, kept separate from the general report so the
# motion vector question has a direct answer.
VELOCITY_FORMATS = {"DXGI_FORMAT_R16G16_FLOAT", "DXGI_FORMAT_R16G16_UNORM",
                    "DXGI_FORMAT_R16G16_SNORM", "DXGI_FORMAT_R32G32_FLOAT",
                    "DXGI_FORMAT_R10G10B10A2_UNORM"}
DEPTH_FORMATS = {"DXGI_FORMAT_R24G8_TYPELESS", "DXGI_FORMAT_D24_UNORM_S8_UINT",
                 "DXGI_FORMAT_R32_TYPELESS", "DXGI_FORMAT_D32_FLOAT",
                 "DXGI_FORMAT_R32G8X24_TYPELESS", "DXGI_FORMAT_D32_FLOAT_S8X24_UINT"}

# RenderDoc already renders the flags as text, so use its spelling rather than re-deriving it.
BIND_LABELS = {"D3D11_BIND_SHADER_RESOURCE": "SRV", "D3D11_BIND_RENDER_TARGET": "RTV",
               "D3D11_BIND_DEPTH_STENCIL": "DSV", "D3D11_BIND_UNORDERED_ACCESS": "UAV",
               "D3D11_BIND_VERTEX_BUFFER": "VERTEX", "D3D11_BIND_INDEX_BUFFER": "INDEX",
               "D3D11_BIND_CONSTANT_BUFFER": "CONSTANT", "D3D11_BIND_STREAM_OUTPUT": "SO"}


def text_of(node, name):
    child = node.find(f"./*[@name='{name}']")
    return child.text if child is not None else None


def enum_of(node, name):
    child = node.find(f"./*[@name='{name}']")
    return child.get("string") if child is not None else None


def describe_binds(value):
    if not value:
        return []
    return [BIND_LABELS.get(part.strip(), part.strip()) for part in value.split("|")]


def parse(path):
    tree = ElementTree.parse(path)
    root = tree.getroot()
    header = root.find("header")
    thumbnail = header.find("thumbnail") if header is not None else None

    textures = {}
    order = []
    counts = Counter()
    for chunk in root.iter("chunk"):
        name = chunk.get("name", "")
        counts[name] += 1
        if name == "ID3D11Device::CreateTexture2D":
            desc = chunk.find("./*[@name='Descriptor']")
            if desc is None:
                continue
            identifier = text_of(chunk, "pTexture")
            sample = desc.find("./*[@name='SampleDesc']")
            textures[identifier] = {
                "id": identifier,
                "width": text_of(desc, "Width"),
                "height": text_of(desc, "Height"),
                "mips": text_of(desc, "MipLevels"),
                "array": text_of(desc, "ArraySize"),
                "format": enum_of(desc, "Format"),
                "samples": text_of(sample, "Count") if sample is not None else None,
                "usage": enum_of(desc, "Usage"),
                "bind": describe_binds(enum_of(desc, "BindFlags")),
            }
        elif name == "ID3D11DeviceContext::OMSetRenderTargets":
            targets = [node.text for node in chunk.iter() if node.get("name") == "ppRenderTargetViews"]
            order.append({"views": targets})
    return thumbnail, textures, order, counts


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("capture_xml", type=Path)
    parser.add_argument("--output", type=Path, default=None)
    args = parser.parse_args()

    thumbnail, textures, order, counts = parse(args.capture_xml)
    if thumbnail is not None:
        print(f"presented size: {thumbnail.get('width')}x{thumbnail.get('height')}")
    print(f"{len(textures)} textures created, {sum(counts.values())} chunks, "
          f"{counts['ID3D11DeviceContext::OMSetRenderTargets']} render target bindings")

    by_size = defaultdict(list)
    for texture in textures.values():
        if "RTV" in texture["bind"] or "DSV" in texture["bind"] or "UAV" in texture["bind"]:
            by_size[(texture["width"], texture["height"])].append(texture)

    print("\nrender targets by resolution:")
    for (width, height), group in sorted(by_size.items(),
                                         key=lambda item: -(int(item[0][0] or 0) * int(item[0][1] or 0))):
        formats = Counter(texture["format"] for texture in group)
        print(f"  {width}x{height}: {len(group)} targets")
        for format_name, count in formats.most_common(8):
            marker = ""
            if format_name in VELOCITY_FORMATS:
                marker = "   <- velocity candidate"
            elif format_name in DEPTH_FORMATS:
                marker = "   <- depth"
            print(f"      {count:>3}  {format_name}{marker}")

    if args.output:
        args.output.write_text(json.dumps(
            {"textures": list(textures.values()), "chunk_counts": counts,
             "render_target_bindings": len(order)}, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()

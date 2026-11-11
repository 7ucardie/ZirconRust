#!/usr/bin/env python3
"""Dress hand-drawn source armour poses with painted fabric (requires Pillow)."""
import argparse
import json
import shutil
from pathlib import Path

from PIL import Image

from mcp_smoke import Client, digest


# (body family, source item shape, hand-drawn pose donor, source item ID)
FAMILIES = (
    ("armour-male", 4, 5, 194),
    ("armour-female", 4, 5, 195),
    ("armour-assassin-male", 1, 2, 130),
    ("armour-assassin-female", 1, 2, 131),
)
TEXTILES = Path(__file__).resolve().parents[1] / "crates/mir-editor/art/garment-textiles.png"


def dress(source, donor, output, textile, assassin):
    """Retarget every donor pose to source identity; retain drawn edges and shade."""
    shutil.copytree(source, output)
    source_manifest = json.loads((output / "project.json").read_text())
    donor_manifest = json.loads((donor / "project.json").read_text())
    assert len(source_manifest["slots"]) == len(donor_manifest["slots"])
    fabric = textile.load()
    frames = pixels_changed = 0
    for target, pose in zip(source_manifest["slots"], donor_manifest["slots"]):
        assert target["present"] == pose["present"], "Pose donor has different slot coverage"
        for field in ("offset_x", "offset_y", "shadow_offset_x", "shadow_offset_y", "shadow_type"):
            target[field] = pose[field]
        for layer in ("image", "shadow", "overlay"):
            ours, theirs = target[layer], pose[layer]
            assert (ours is None) == (theirs is None), "Pose donor has different layers"
            if theirs is None:
                continue
            ours["width"], ours["height"] = theirs["width"], theirs["height"]
            source_png = donor / theirs["path"]
            output_png = output / ours["path"]
            if layer != "image":
                shutil.copyfile(source_png, output_png)
                continue
            image = Image.open(source_png).convert("RGBA")
            assert image.size == (ours["width"], ours["height"])
            rgba = image.load()
            for y in range(image.height):
                for x in range(image.width):
                    red, green, blue, alpha = rgba[x, y]
                    if alpha == 0:
                        continue
                    light = (red + green + blue) // 3
                    if max(red, green, blue) - min(red, green, blue) > 28 or light < (25 if assassin else 52):
                        continue
                    ink = fabric[(x + pose["offset_x"]) % textile.width,
                                 (y + pose["offset_y"]) % textile.height]
                    shade = 0.42 + light / 300
                    rgba[x, y] = tuple(min(255, round(channel * shade * 0.82 + old * 0.18))
                                       for channel, old in zip(ink, (red, green, blue))) + (alpha,)
                    pixels_changed += 1
            image.save(output_png)
            frames += 1
    assert frames > 0 and pixels_changed > frames, "No painted fabric reached source poses"
    (output / "project.json").write_text(json.dumps(source_manifest, indent=2) + "\n")
    return frames, pixels_changed


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("binary", "assets", "db", "project"):
        parser.add_argument("--" + name, type=Path, required=True)
    args = parser.parse_args()
    for name in ("binary", "assets", "db", "project"):
        setattr(args, name, getattr(args, name).resolve())
    args.project.mkdir()
    atlas = Image.open(TEXTILES).convert("RGB")
    assert atlas.width % 2 == atlas.height % 2 == 0, "Textile atlas needs four equal panels"
    textiles = [atlas.crop((column * atlas.width // 2, row * atlas.height // 2,
                           (column + 1) * atlas.width // 2, (row + 1) * atlas.height // 2))
                .resize((64, 96), Image.Resampling.LANCZOS)
                for row, column in ((0, 0), (0, 1), (1, 0), (1, 1))]
    sources = [args.db] + [args.assets / "Data" / name for name in (
        "M-Hum.Zl", "WM-Hum.Zl", "M-HumA.Zl", "WM-HumA.Zl",
        "M-HumEx1.Zl", "WM-HumEx1.Zl", "M-HumAEx1.Zl", "WM-HumAEx1.Zl")]
    before = [digest(path) for path in sources]
    reports = []
    with (args.project / "stderr.log").open("w") as log:
        client = Client(args, log)
        try:
            client.rpc("initialize", {"protocolVersion": "2025-11-25", "capabilities": {},
                "clientInfo": {"name": "outfit-demo", "version": "1"}})
            client.send("notifications/initialized", {})
            for textile, (kind, source_shape, donor_shape, item) in zip(textiles, FAMILIES):
                source_name, donor_name = kind, kind + "-donor"
                for shape, output in ((source_shape, source_name), (donor_shape, donor_name)):
                    client.call("sprite_export_project", {"kind": kind, "source_image": shape,
                        "output": output})
                output = kind + "-outfit"
                frames, painted = dress(args.project / source_name, args.project / donor_name,
                                        args.project / output, textile, "assassin" in kind)
                project = f"project:{output}/project.json"
                client.call("sprite_validate_project", {"project": project})
                published = client.call("wearable_publish", {"projects": [project],
                    "source_id": item, "name": "Painted Mantle " + kind,
                    "output": kind + "-content"})
                reports.append({"family": kind, "donor_shape": donor_shape,
                    "drawn_frames": frames, "textured_pixels": painted, "published": published})
            assert [digest(path) for path in sources] == before, "Source assets changed"
            (args.project / "report.json").write_text(json.dumps({"status": "passed",
                "textile_atlas_sha256": digest(TEXTILES), "families": reports}, indent=2) + "\n")
            client.close()
            print(json.dumps({"status": "passed", "families": len(reports),
                "drawn_frames": sum(report["drawn_frames"] for report in reports)}))
        finally:
            if client.process.poll() is None:
                client.process.kill()
                client.process.wait()


if __name__ == "__main__":
    main()

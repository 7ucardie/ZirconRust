#!/usr/bin/env python3
"""Build one complete male Azure Wayfarer wearable in a private asset view."""
import argparse
import json
import shutil
from pathlib import Path

from PIL import Image

from mcp_smoke import Client, digest
from sprite_outfit_demo import TEXTILES, dress


ART = Path(__file__).resolve().parents[1] / "crates/mir-editor/art/azure-wayfarer"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("binary", "assets", "db", "project"):
        parser.add_argument("--" + name, type=Path, required=True)
    args = parser.parse_args()
    for name in ("binary", "assets", "db", "project"):
        setattr(args, name, getattr(args, name).resolve())
    if args.project.exists():
        parser.error("--project must be a fresh, nonexistent directory")
    args.project.mkdir(parents=True)
    for name in ("walk-strip.png", "icon.png"):
        shutil.copyfile(ART / name, args.project / name)
    with Image.open(TEXTILES) as atlas:
        atlas = atlas.convert("RGB")
        assert atlas.width % 2 == atlas.height % 2 == 0
        textile = atlas.crop((0, 0, atlas.width // 2, atlas.height // 2)).resize(
            (64, 96), Image.Resampling.LANCZOS
        )
    sources = [args.db] + [args.assets / "Data" / name for name in (
        "M-Hum.Zl", "M-HumEx1.Zl", "Ground.Zl", "Inventory.Zl", "Storeitems.Zl")]
    before = [digest(path) for path in sources]
    with (args.project / "mcp.log").open("w") as log:
        client = Client(args, log)
        try:
            client.rpc("initialize", {"protocolVersion": "2025-11-25", "capabilities": {},
                                      "clientInfo": {"name": "azure-wayfarer", "version": "1"}})
            client.send("notifications/initialized", {})
            for shape, name in ((4, "source"), (5, "pose-donor")):
                client.call("sprite_export_project", {"kind": "armour-male",
                            "source_image": shape, "output": name})
            frames, painted = dress(args.project / "source", args.project / "pose-donor",
                                    args.project / "body-base", textile, False)
            mapping = [{"slot": slot, "layer": "image", "column": slot - 100, "row": 0}
                       for slot in range(100, 106)]
            client.call("sprite_import_sheet", {
                "project": "project:body-base/project.json", "image": "project:walk-strip.png",
                "cell_width": 362, "cell_height": 724, "mapping": mapping,
                "fit": True, "binary_alpha": True, "output": "body"})
            assert all(digest(args.project / "body/frames" / f"{slot:06}-image.png") !=
                       digest(args.project / "body-base/frames" / f"{slot:06}-image.png")
                       for slot in range(100, 106))
            checked = client.call("sprite_validate_project", {
                "project": "project:body/project.json"})
            preview = client.call("sprite_preview_sheet", {
                "project": "project:body/project.json", "output": "preview"})
            built = client.call("sprite_build_pack", {
                "project": "project:body/project.json", "output": "build"})
            client.call("sprite_export_project", {"kind": "item",
                        "source_image": 1020, "output": "icon-source"})
            client.call("sprite_import_image", {
                "project": "project:icon-source/project.json", "image": "project:icon.png",
                "slot": 0, "layer": "image", "fit": True, "binary_alpha": True,
                "trim_transparent": True, "canvas": [40, 40], "offsets": [-20, -20],
                "output": "icon-art"})
            client.call("sprite_validate_project", {"project": "project:icon-art/project.json"})
            client.call("sprite_build_pack", {
                "project": "project:icon-art/project.json", "output": "icon-build"})
            icon = client.call("sprite_publish", {
                "project": "project:icon-art/project.json", "kind": "item",
                "source_id": 194, "name": "Azure Wayfarer Coat Icon Base",
                "output": "icon-published"})
            published = client.call("wearable_publish", {
                "projects": ["project:body/project.json"],
                "source_id": icon["created_id"], "name": "Azure Wayfarer Coat",
                "database": "project:icon-published/System.db",
                "assets": "project:icon-published/Client", "output": "published"})
            assert [digest(path) for path in sources] == before, "installed assets changed"
            report = {"status": "passed", "painted_frames": frames,
                      "painted_pixels": painted, "generated_walk_frames": 6,
                      "validated": checked, "preview": preview, "build": built,
                      "icon": icon, "published": published}
            (args.project / "report.json").write_text(json.dumps(report, indent=2) + "\n")
            print(json.dumps({"status": "passed", "frames": frames,
                              "item_id": published["created_id"],
                              "assets": published["assets"],
                              "database": published["database"]}))
        finally:
            if client.process.poll() is None:
                client.process.kill()
                client.process.wait()


if __name__ == "__main__":
    main()

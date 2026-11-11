#!/usr/bin/env python3
"""Exercise sprite authoring tools against a real stdio MCP process."""
import argparse
import json
from pathlib import Path
import shutil
from mcp_smoke import Client, digest


def smoke(args, client):
    initialized = client.rpc("initialize", {"protocolVersion": "2025-11-25",
        "capabilities": {}, "clientInfo": {"name": "zircon-sprite-smoke", "version": "1"}})
    assert initialized["protocolVersion"] == "2025-11-25"
    client.send("notifications/initialized", {})
    assert client.rpc("ping", {}) == {}
    tools = {entry["name"] for entry in client.rpc("tools/list", {})["tools"]}
    expected = {"sprite_export_project", "sprite_reference", "sprite_validate_project",
                "sprite_build_pack", "sprite_preview_sheet", "sprite_import_image",
                "sprite_import_sheet", "sprite_publish", "sprite_paint_motion",
                "sprite_auto_motion", "sprite_pose_from_reference"}
    assert expected <= tools
    swords = client.call("content_search", {"kind": "items", "filter": "Sword", "limit": 100})["entries"]
    sword = next((entry for entry in swords if entry["name"] == "WoodenSword"), swords[0])
    client.call("sprite_export_project", {"kind": "item", "source_image": sword["image"], "output": "item-source"})
    manifest = json.loads((args.project / "item-source/project.json").read_text())
    source = Path(manifest["source_path"])
    before = digest(source)
    source_project = digest(args.project / "item-source/project.json")
    shutil.copyfile(args.art, args.project / "art.png")
    client.call("sprite_import_image", {"project": "project:item-source/project.json", "image": "project:art.png",
                "slot": 0, "layer": "image", "fit": True, "binary_alpha": True, "output": "item-edit"})
    checked = client.call("sprite_validate_project", {"project": "project:item-edit/project.json"})
    assert checked["changed_slots"] == 1
    edited = json.loads((args.project / "item-edit/project.json").read_text())
    image = edited["slots"][0]["image"]
    client.call("sprite_import_sheet", {"project": "project:item-edit/project.json",
        "image": "project:item-edit/" + image["path"], "cell_width": image["width"],
        "cell_height": image["height"], "mapping": [{"slot": 0, "layer": "image", "column": 0, "row": 0}],
        "output": "item-sheet"})
    client.call("sprite_preview_sheet", {"project": "project:item-sheet/project.json", "output": "preview"})
    assert (args.project / "preview/preview.png").is_file()
    built = client.call("sprite_build_pack", {"project": "project:item-sheet/project.json", "output": "built"})
    assert Path(built["library"]).is_file()
    published = client.call("sprite_publish", {"project": "project:item-sheet/project.json", "kind": "item",
        "source_id": sword["id"], "name": "Jade Beetle Talisman Smoke", "output": "published"})
    assert published["image"] != sword["image"]
    assert Path(published["database"]).is_file() and Path(published["assets"]).is_dir()
    found = client.call("content_search", {"kind": "items", "filter": "Jade Beetle Talisman Smoke",
        "database": "project:published/System.db"})["entries"]
    assert any(entry["id"] == published["created_id"] and entry["image"] == published["image"] for entry in found)
    client.call("sprite_export_project", {"kind": "armour-male", "source_image": 4,
        "output": "body-source"})
    body = json.loads((args.project / "body-source/project.json").read_text())
    body_source_hash = digest(Path(body["source_path"]))
    posed = client.call("sprite_pose_from_reference", {"kind": "armour-male",
        "source_image": 4, "donor_image": 5, "output": "body-reference-posed"})
    assert posed["changed_slots"] > 1000
    assert client.call("sprite_validate_project",
        {"project": "project:body-reference-posed/project.json"})["changed_slots"] == posed["changed_slots"]
    posed_pack = client.call("sprite_build_pack", {"project": "project:body-reference-posed/project.json",
        "output": "body-reference-pack"})
    assert Path(posed_pack["library"]).is_file()
    contract = json.loads((args.project / "body-source/contract.json").read_text())
    frames = contract["frames"]
    run = next(frames[i:i + 3] for i in range(len(frames) - 2)
        if frames[i]["action"] == "Walking" and frames[i]["direction"] == 4
        and all(frames[i + j]["action"] == frames[i]["action"]
               and frames[i + j]["direction"] == frames[i]["direction"]
               and frames[i + j]["frame"] == frames[i]["frame"] + j
               and frames[i + j]["index"] == frames[i]["index"] + j
               for j in range(3)))
    slots = [frame["index"] - body["source_start"] for frame in run]
    motion = client.call("sprite_paint_motion", {"project": "project:body-source/project.json",
        "output": "body-motion", "padding": [4, 0, 4, 0], "slots": slots,
        "keyframes": [
            {"slot": slots[0], "points": [[-3, 8], [-1, 8], [-1, 16], [-3, 16]],
             "color": [94, 43, 140, 255], "under": True},
            {"slot": slots[-1], "points": [[-1, 8], [1, 8], [1, 16], [-1, 16]],
             "color": [120, 54, 160, 255], "under": True}]})
    assert motion["changed_slots"] == 3
    assert client.call("sprite_validate_project",
        {"project": "project:body-motion/project.json"})["changed_slots"] == 3
    designed = client.call("sprite_auto_motion", {"project": "project:body-source/project.json",
        "output": "body-designed", "padding": [4, 0, 4, 0], "slots": slots,
        "seed": {"slot": slots[1], "points": [[9, 20], [22, 20], [27, 62], [4, 62]],
                 "color": [94, 43, 140, 255], "under": True}, "strength": 4})
    assert designed["changed_slots"] == 3
    assert client.call("sprite_validate_project",
        {"project": "project:body-designed/project.json"})["changed_slots"] == 3
    assert digest(Path(body["source_path"])) == body_source_hash
    client.call("sprite_reference", {"kind": "monster", "source_image": 33, "output": "monster-source"})
    monster_project = json.loads((args.project / "monster-source/project.json").read_text())
    assert len(monster_project["slots"]) == 1000
    monster_validation = client.rpc("tools/call", {"name": "sprite_validate_project",
        "arguments": {"project": "project:monster-source/project.json"}})
    assert monster_validation.get("isError")
    assert "Missing required animation image slots" in monster_validation["content"][0]["text"]
    assert client.call("sprite_build_pack", {"project": "project:monster-source/project.json",
        "output": "monster-built"}, allow_error=True) is None
    assert not (args.project / "monster-built").exists()
    error = client.call("sprite_import_image", {"project": "project:item-source/project.json",
        "image": "project:../art.png", "slot": 0, "layer": "image", "output": "escape"}, allow_error=True)
    assert error is None and not (args.project / "escape").exists()
    assert digest(source) == before and digest(args.project / "item-source/project.json") == source_project
    return {"status": "passed", "sprite_tools": sorted(expected), "rpc_requests": client.next_id,
            "published": published, "incomplete_source_monster_build_refused": True, "project": str(args.project)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["binary", "assets", "db", "project"]:
        parser.add_argument("--" + name, required=True, type=lambda p: Path(p).expanduser().resolve())
    parser.add_argument("--art", type=lambda p: Path(p).expanduser().resolve(),
        default=Path(__file__).resolve().parents[1] / "assets/authoring-demo/jade-beetle-talisman.png")
    args = parser.parse_args()
    if args.project.exists():
        parser.error("--project must be a fresh, nonexistent directory")
    source_paths = [args.db]
    for name in ["storeitems.zl", "inventory.zl", "ground.zl"]:
        source_paths.append(next(path for path in (args.assets / "Data").iterdir() if path.name.lower() == name))
    original_hashes = [digest(path) for path in source_paths]
    args.project.mkdir()
    client = None
    try:
        with (args.project / "sprite-smoke.stderr.log").open("w") as stderr:
            client = Client(args, stderr)
            report = smoke(args, client)
            client.close()
    finally:
        if client is not None and client.process.poll() is None:
            client.process.kill()
            client.process.wait(timeout=10)
        assert [digest(path) for path in source_paths] == original_hashes, "Source database or item libraries changed"
    (args.project / "sprite-smoke-report.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()

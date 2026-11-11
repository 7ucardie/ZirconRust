#!/usr/bin/env python3
"""Exercise wearable body contracts and private publication over real MCP stdio."""
import argparse
import json
from pathlib import Path
from mcp_smoke import Client, digest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("binary", "assets", "db", "project"):
        parser.add_argument("--" + name, type=Path, required=True)
    args = parser.parse_args()
    for name in ("binary", "assets", "db", "project"):
        setattr(args, name, getattr(args, name).resolve())
    args.project.mkdir()
    before = digest(args.db)
    reports = []
    with (args.project / "stderr.log").open("w") as log:
        client = Client(args, log)
        try:
            client.rpc("initialize", {"protocolVersion": "2025-11-25", "capabilities": {},
                "clientInfo": {"name": "wearable-smoke", "version": "1"}})
            client.send("notifications/initialized", {})
            for kind, shape, source in [("armour-male", 4, 194), ("armour-female", 4, 195),
                    ("armour-assassin-male", 1, 130), ("armour-assassin-female", 1, 131)]:
                client.call("sprite_export_project", {"kind": kind, "source_image": shape, "output": kind})
                project = "project:" + kind + "/project.json"
                contract = json.loads((args.project / kind / "contract.json").read_text())
                assert not contract["missing_required"], contract["missing_required"]
                client.call("sprite_validate_project", {"project": project})
                tinted = client.call("sprite_tint", {"project": project, "layer": "image", "rgb": [120, 220, 255], "output": kind + "-tinted"})
                assert tinted["changed_slots"] > 0
                project = "project:" + kind + "-tinted/project.json"
                result = client.call("wearable_publish", {"projects": [project], "source_id": source,
                    "name": "Authored " + kind, "output": kind + "-content"})
                assert result["shape"] != shape
                reports.append(result)
            assert client.call("wearable_publish", {"projects": ["project:armour-female/project.json"],
                "source_id": 194, "name": "Wrong body", "output": "must-not-exist"}, allow_error=True) is None
            assert not (args.project / "must-not-exist").exists()
            projects = ["project:" + report["body_families"][0] + "-tinted/project.json" for report in reports]
            batch = client.call("sprite_build_batch", {"projects": projects, "output": "body-packs"})
            assert len(batch["manifest"]["packs"]) == 4
            for pack in batch["manifest"]["packs"]:
                assert (args.project / "body-packs" / pack["pack"]).is_file()
            assert client.call("sprite_build_batch", {"projects": [projects[0], projects[0]], "output": "bad-batch"}, allow_error=True) is None
            assert not (args.project / "bad-batch").exists()
            assert digest(args.db) == before
            (args.project / "report.json").write_text(json.dumps({"status": "passed", "publications": reports}, indent=2) + "\n")
            client.close()
            print(json.dumps({"status": "passed", "families": len(reports), "requests": client.next_id}))
        finally:
            if client.process.poll() is None:
                client.process.kill()
                client.process.wait(timeout=10)


if __name__ == "__main__":
    main()

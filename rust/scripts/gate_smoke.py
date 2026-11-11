#!/usr/bin/env python3
"""Create, preview and relocate a private stock-art castle gate through MCP."""
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
    with (args.project / "stderr.log").open("w") as log:
        client = Client(args, log)
        try:
            client.rpc("initialize", {"protocolVersion": "2025-11-25", "capabilities": {},
                "clientInfo": {"name": "gate-smoke", "version": "1"}})
            client.send("notifications/initialized", {})
            source = client.call("content_search", {"kind": "monsters", "filter": "Scarecrow"})["entries"][0]
            placement = {"castle": 1, "monster": source["id"], "gate_image": 543,
                "x": 23, "y": 108, "repair_cost": 1000}
            preview = client.call("castle_gate_preview", {"placement": placement})
            assert len(preview["closed_cells"]) == 6
            created = client.call("castle_gate_place", {"placement": placement, "output": "created"})
            assert created["gate"]["monster"] != source["id"]
            assert client.call("castle_gate_place", {"placement": placement, "database": "project:created/System.db",
                "output": "overlap"}, allow_error=True) is None
            assert not (args.project / "overlap").exists()
            relocation = {"gate": created["gate"]["gate"], "x": 25, "y": 108, "repair_cost": 0}
            client.call("castle_gate_preview", {"placement": relocation, "database": "project:created/System.db"})
            moved = client.call("castle_gate_place", {"placement": relocation, "database": "project:created/System.db", "output": "moved"})
            assert moved["gate"]["repair_cost"] == 1000
            assert moved["gate"]["monster"] == created["gate"]["monster"]
            client.call("sprite_export_project", {"kind": "monster", "source_image": 543,
                "output": "gate-sprite"})
            contract = json.loads((args.project / "gate-sprite/contract.json").read_text())
            assert not contract["missing_required"]
            published = client.call("sprite_publish", {"project": "project:gate-sprite/project.json",
                "kind": "monster", "source_id": created["gate"]["monster"],
                "database": "project:created/System.db", "name": "Gate Sprite Smoke", "output": "published"})
            assert 540 <= published["image"] <= 543 and published["image"] != 543
            assert digest(args.db) == before
            (args.project / "report.json").write_text(json.dumps({"status": "passed", "created": created, "moved": moved}, indent=2) + "\n")
            client.close()
            print(json.dumps({"status": "passed", "requests": client.next_id}))
        finally:
            if client.process.poll() is None:
                client.process.kill()
                client.process.wait(timeout=10)


if __name__ == "__main__":
    main()

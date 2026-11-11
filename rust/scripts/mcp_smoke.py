#!/usr/bin/env python3
"""Exercise the real editor MCP process using only Python's standard library."""
import argparse
import hashlib
import json
from pathlib import Path
import queue
import subprocess
import threading


def digest(path):
    checksum = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            checksum.update(block)
    return checksum.hexdigest()


class Client:
    def __init__(self, args, stderr):
        self.process = subprocess.Popen(
            [str(args.binary), "--mcp", "--project", str(args.project),
             "--assets", str(args.assets), "--db", str(args.db)],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=stderr,
            text=True, encoding="utf-8", bufsize=1,
        )
        self.lines = queue.Queue()
        self.next_id = 0
        self.reader = threading.Thread(target=self.read, daemon=True)
        self.reader.start()

    def read(self):
        try:
            for line in self.process.stdout:
                self.lines.put(line)
        finally:
            self.lines.put(None)

    def send(self, method, params, request_id=None):
        message = {"jsonrpc": "2.0", "method": method, "params": params}
        if request_id is not None:
            message["id"] = request_id
        self.process.stdin.write(json.dumps(message, separators=(",", ":")) + "\n")
        self.process.stdin.flush()

    def rpc(self, method, params):
        self.next_id += 1
        self.send(method, params, self.next_id)
        line = self.lines.get(timeout=60)
        if line is None:
            raise RuntimeError("MCP server closed stdout before responding")
        response = json.loads(line)  # Any stray logging on stdout fails here.
        assert response["jsonrpc"] == "2.0" and response["id"] == self.next_id
        if "error" in response:
            raise RuntimeError(f"{method}: {response['error']}")
        return response["result"]

    def call(self, name, arguments, allow_error=False):
        result = self.rpc("tools/call", {"name": name, "arguments": arguments})
        if result.get("isError"):
            if allow_error:
                return None
            raise RuntimeError(f"{name}: {result['content']}")
        structured = result["structuredContent"]
        assert isinstance(structured, dict)
        assert json.loads(result["content"][0]["text"]) == structured
        return structured

    def close(self):
        self.process.stdin.close()
        try:
            assert self.process.wait(timeout=10) == 0
            self.reader.join(timeout=10)
            assert not self.reader.is_alive()
            assert self.lines.get(timeout=1) is None, "Unexpected output after final response"
            assert self.lines.empty()
        finally:
            self.process.stdout.close()


def verify_bundle(client, project, revision, monster, item):
    folder = project / revision
    maps = list((folder / "Map").glob("*.map"))
    assert len(maps) == 1
    map_path = maps[0]
    layout, selected = json.loads(map_path.with_suffix(".cave.json").read_text())
    navigation = json.loads(map_path.with_suffix(".navigation.json").read_text())
    assert selected == monster and len(layout["spawns"]) >= 8
    assert navigation["portals"][0]["position"] == layout["exits"][0]
    assert navigation["portals"][0]["destination"] == layout["landings"][1]
    # Navigation uses FNV-1a of the serialized map; catch stale copied sidecars.
    checksum = 0xCBF29CE484222325
    for byte in map_path.read_bytes():
        checksum = ((checksum ^ byte) * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    assert navigation["map_checksum"] == f"{checksum:016x}"
    reference = f"project:{revision}/Map/{map_path.name}"
    inspection = client.call("map_inspect", {"map": reference,
        "points": [layout["start"], *layout["exits"], *layout["landings"]]})
    assert all(cell["walkable"] for cell in inspection["cells"])
    database = f"project:{revision}/System.db"
    for kind, name, expected in [("monsters", "MCP Smoke Chicken", monster),
                                 ("items", "MCP Smoke Sword", item)]:
        entries = client.call("content_search", {"kind": kind, "filter": name,
                              "database": database})["entries"]
        assert any(entry["id"] == expected for entry in entries)
    assert (folder / "README.txt").is_file()
    return reference, layout


def smoke(args, client):
    initialized = client.rpc("initialize", {"protocolVersion": "2025-11-25",
        "capabilities": {}, "clientInfo": {"name": "zircon-mcp-smoke", "version": "1"}})
    assert initialized["protocolVersion"] == "2025-11-25"
    client.send("notifications/initialized", {})
    assert client.rpc("ping", {}) == {}
    prompts = client.rpc("prompts/list", {})["prompts"]
    assert any(prompt["name"] == "create_world" for prompt in prompts)
    rendered = client.rpc("prompts/get", {"name": "create_world",
        "arguments": {"request": "A mossy Wood maze with six rooms and chickens"}})
    assert "six rooms and chickens" in rendered["messages"][0]["content"]["text"]
    assert "world_branch" in rendered["messages"][0]["content"]["text"]
    tools = {tool["name"] for tool in client.rpc("tools/list", {})["tools"]}
    assert {"content_search", "draft_variant", "map_inspect", "map_create",
            "map_copy", "map_copy_area", "map_collision", "map_stamp", "world_generate",
            "world_perimeter", "world_branch", "room_door_place",
            "world_populate", "world_spawn"} <= tools
    chicken = next(entry for entry in client.call("content_search",
        {"kind": "monsters", "filter": "Chicken"})["entries"] if entry["name"] == "Chicken")
    monster = client.call("draft_variant", {"kind": "monster", "source_id": chicken["id"],
        "level": 20, "name": "MCP Smoke Chicken", "output": "monster"})["created_id"]
    swords = client.call("content_search", {"kind": "items", "filter": "Sword", "limit": 100})["entries"]
    item = None
    for sword in swords:
        created = client.call("draft_variant", {"kind": "item", "source_id": sword["id"],
            "level": 30, "name": "MCP Smoke Sword", "output": "item",
            "database": "project:monster/System.db"}, allow_error=True)
        if created:
            item = created["created_id"]
            break
    assert item is not None, "No draftable Sword item found in installed database"
    client.call("map_create", {"output": "blank", "width": 64, "height": 64})
    original = digest(args.project / "blank/Map.map")
    client.call("map_collision", {"map": "project:blank/Map.map", "output": "blocked",
        "points": [[1, 1]], "walkable": False})
    assert digest(args.project / "blank/Map.map") == original
    inspection = client.call("map_inspect", {"map": "project:blocked/Map.map", "points": [[1, 1]]})
    assert not inspection["cells"][0]["walkable"]
    assert client.call("map_create", {"output": "blank", "width": 4, "height": 4}, allow_error=True) is None
    client.call("map_copy", {"map": "project:blocked/Map.map", "output": "copied"})
    client.call("map_stamp", {"map": "project:copied/Map.map", "output": "stamped",
        "kit": "wood-north", "x": 14, "y": 2})
    client.call("map_copy_area", {"source": "asset:D1006.map", "map": "project:stamped/Map.map",
        "bounds": [14, 2, 16, 14], "x": 14, "y": 2, "output": "area"})
    client.call("map_create", {"output": "entry", "width": 64, "height": 64,
        "kit": "wood-entry", "x": 4, "y": 4})
    client.call("world_generate", {"source": "asset:D1006.map", "mode": "chamber",
        "width": 80, "height": 80, "seed": 42, "monster_id": monster,
        "output": "world", "database": "project:item/System.db"})
    reference, layout = verify_bundle(client, args.project, "world", monster, item)
    for mode in ["organic", "rooms"]:
        created = client.call("world_generate", {"source": "asset:D1008.map",
            "mode": mode, "width": 80, "height": 80, "seed": 42,
            "monster_id": monster, "output": f"full_art_{mode}",
            "database": "project:item/System.db"})
        assert created["wall_cells"] > 100
        assert Path(created["assets"]).joinpath("Data/Map Data/Wood/Dungeonsc.Zl").is_file()
        verify_bundle(client, args.project, f"full_art_{mode}", monster, item)
    client.call("world_populate", {"map": reference, "seed": 99, "monster_id": monster,
        "output": "populated", "database": "project:world/System.db"})
    _, populated = verify_bundle(client, args.project, "populated", monster, item)
    assert populated == layout and populated["enclosed"]
    # Copy invokes server-side layout/navigation validation before creating files.
    client.call("map_copy", {"map": reference, "output": "world_copy"})
    assert json.loads((args.project / "world_copy/Map.cave.json").read_text())[0] == layout
    perimeter = client.call("world_perimeter", {"nw": 3, "ne": 4, "seed": 23,
        "monster_id": monster, "output": "perimeter", "database": "project:item/System.db"})
    assert perimeter["wall_repeats"] == [3, 4] and perimeter["enclosed"]
    perimeter_reference, perimeter_layout = verify_bundle(client, args.project, "perimeter", monster, item)
    assert perimeter_layout["wall_repeats"] == [3, 4] and perimeter_layout["enclosed"]
    client.call("world_populate", {"map": perimeter_reference, "seed": 99,
        "monster_id": monster, "output": "perimeter_populated", "database": "project:perimeter/System.db"})
    _, repopulated = verify_bundle(client, args.project, "perimeter_populated", monster, item)
    assert repopulated == perimeter_layout
    connected = client.call("world_perimeter", {"rooms": 2, "nw": 3, "ne": 4, "seed": 23,
        "monster_id": monster, "output": "connected", "database": "project:item/System.db"})
    assert connected["wall_rooms"] == 2 and connected["enclosed"]
    _, connected_layout = verify_bundle(client, args.project, "connected", monster, item)
    assert connected_layout["wall_rooms"] == 2 and connected_layout["wall_repeats"] == [3, 4]
    door = client.call("room_door_place", {"map": "project:connected/Map/GeneratedCave.map",
        "database": "project:connected/System.db", "center": connected["doorways"][0]["center"],
        "source_monster": monster, "output": "room_door"})
    assert door["doors"] == 1 and (args.project / "room_door/Map/GeneratedCave.doors.json").is_file()
    assert Path(door["assets"]).joinpath("Data/Mon-54.Zl").is_file()
    populated_door = client.call("world_populate", {"map": door["map"],
        "database": door["database"], "seed": 23, "monster_id": monster,
        "output": "room_door_populated"})
    assert Path(populated_door["assets"]).joinpath("Data/Mon-54.Zl").is_file()
    assert list((args.project / "room_door_populated/Map").glob("*.doors.json"))
    for tool, options in [
        ("world_generate", {"source": "asset:D1006.map", "mode": "chamber",
                            "width": 80, "height": 80}),
        ("world_perimeter", {"nw": 3, "ne": 4}),
        ("world_populate", {"map": perimeter_reference}),
    ]:
        revision = f"art_{tool}"
        result = client.call(tool, {**options, "seed": 23, "monster_id": monster,
            "output": revision, "database": door["database"]})
        assert digest(Path(result["assets"]) / "Data/Mon-54.Zl") == digest(Path(door["assets"]) / "Data/Mon-54.Zl")
    branch = client.call("world_branch", {"arms": [2, 5, 8], "seed": 23,
        "monster_id": monster, "output": "branch", "database": "project:item/System.db"})
    assert branch["arms"] == [2, 5, 8] and len(branch["centers"]) == 4
    assert (args.project / "branch/Client/Data/Map Data/Wood/Dungeonsc.Zl").is_file()
    verify_bundle(client, args.project, "branch", monster, item)
    generated_from_branch = client.call("world_generate", {
        "source": "project:branch/Map/GeneratedCave.map", "mode": "chamber",
        "width": 120, "height": 120, "seed": 42, "monster_id": monster,
        "output": "generated_from_branch", "database": "project:item/System.db"})
    assert digest(Path(generated_from_branch["assets"]) / "Data/Map Data/Wood/Dungeonsc.Zl") == digest(Path(branch["assets"]) / "Data/Map Data/Wood/Dungeonsc.Zl")
    inherited = client.call("world_branch", {"seed": 23, "monster_id": monster,
        "output": "branch_inherited", "database": door["database"]})
    assert digest(Path(inherited["assets"]) / "Data/Mon-54.Zl") == digest(Path(door["assets"]) / "Data/Mon-54.Zl")
    assert client.call("world_generate", {"source": "project:branch/Map/GeneratedCave.map",
        "mode": "chamber", "width": 120, "height": 120, "seed": 42,
        "monster_id": monster, "output": "ambiguous_art", "database": door["database"]},
        allow_error=True) is None
    assert not (args.project / "ambiguous_art").exists()
    combined = client.call("world_generate", {"source": "project:branch/Map/GeneratedCave.map",
        "mode": "chamber", "width": 120, "height": 120, "seed": 42,
        "monster_id": monster, "output": "combined_art", "database": door["database"],
        "assets": "project:branch_inherited/Client"})
    assert digest(Path(combined["assets"]) / "Data/Mon-54.Zl") == digest(Path(door["assets"]) / "Data/Mon-54.Zl")
    assert digest(Path(combined["assets"]) / "Data/Map Data/Wood/Dungeonsc.Zl") == digest(Path(inherited["assets"]) / "Data/Map Data/Wood/Dungeonsc.Zl")
    y = client.call("world_branch", {"shape": "y", "arms": [3, 4, 5], "organic": True,
        "seed": 23, "monster_id": monster, "output": "y_branch",
        "database": "project:item/System.db"})
    assert len(y["centers"]) == 4
    verify_bundle(client, args.project, "y_branch", monster, item)
    graph = client.call("world_branch", {"nodes": [[0, 0], [1, 0], [2, 0], [0, 1], [1, 1], [2, 1]],
        "links": [[0, 1], [1, 2], [2, 5], [5, 4], [4, 3], [3, 0], [1, 4]],
        "organic": True, "seed": 23, "monster_id": monster, "output": "graph",
        "database": "project:item/System.db"})
    assert len(graph["centers"]) == 6
    verify_bundle(client, args.project, "graph", monster, item)
    nodes = [[x, y] for y in range(1, 4) for x in range(4)]
    links = [[4 * y + x, 4 * y + x + 1] for y in range(3) for x in range(3)]
    links += [[4 * y + x, 4 * (y + 1) + x] for y in range(2) for x in range(4)]
    large_graph = client.call("world_branch", {"nodes": nodes, "links": links,
        "organic": True, "seed": 23, "monster_id": monster,
        "output": "large_graph", "database": "project:item/System.db"})
    assert len(large_graph["centers"]) == 12 and large_graph["width"] == 176
    verify_bundle(client, args.project, "large_graph", monster, item)
    filename = Path(perimeter_reference).stem
    registered = client.call("content_search", {"kind": "maps", "filter": filename,
        "database": "project:perimeter/System.db"})["entries"]
    map_id = next(entry["id"] for entry in registered if entry["name"] == filename)
    spawned = client.call("world_spawn", {"map": perimeter_reference, "map_id": map_id,
        "database": "project:perimeter/System.db", "monster_id": monster,
        "points": perimeter_layout["spawns"][:2], "count": 3, "delay_seconds": 45,
        "output": "spawn"})
    assert spawned["region_id"] > 0 and spawned["spawn_id"] > 0
    assert spawned["map"] == perimeter_reference
    assert list((args.project / "spawn").iterdir()) == [args.project / "spawn/System.db"]
    assert Path(spawned["launch"]["ZIRCON_MAP_DIR"]).is_dir()
    private_draft = client.call("draft_variant", {"kind": "monster", "source_id": chicken["id"],
        "level": 20, "name": "Private Art Chicken", "output": "private_draft",
        "database": door["database"]})
    assert digest(Path(private_draft["assets"]) / "Data/Mon-54.Zl") == digest(Path(door["assets"]) / "Data/Mon-54.Zl")
    door_map_id = next(entry["id"] for entry in client.call("content_search", {
        "kind": "maps", "filter": "GeneratedCave", "database": door["database"]})["entries"]
        if entry["name"] == "GeneratedCave")
    private_spawn = client.call("world_spawn", {"map": door["map"], "map_id": door_map_id,
        "database": door["database"], "monster_id": monster,
        "points": connected_layout["spawns"][:2], "count": 3,
        "delay_seconds": 45, "output": "private_spawn"})
    assert private_spawn["launch"]["ZIRCON_ASSETS"] == private_spawn["assets"]
    assert digest(Path(private_spawn["assets"]) / "Data/Mon-54.Zl") == digest(Path(door["assets"]) / "Data/Mon-54.Zl")
    return {"status": "passed", "tools": sorted(tools), "monster_id": monster,
            "item_id": item, "project": str(args.project), "rpc_requests": client.next_id}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["binary", "assets", "db", "project"]:
        parser.add_argument(f"--{name}", required=True, type=lambda path: Path(path).expanduser().resolve())
    args = parser.parse_args()
    if args.project.exists():
        parser.error("--project must be a fresh, nonexistent directory")
    source_paths = [args.db, args.assets / "Map/D1006.map", args.assets / "Map/D1008.map",
                    args.assets / "Map/D10032.map",
                    args.assets / "Data/Map Data/Wood/Dungeonsc.Zl",
                    args.assets / "Data/Mon-54.Zl"]
    before = [digest(path) for path in source_paths]
    args.project.mkdir()
    client = None
    try:
        with (args.project / "smoke.stderr.log").open("w") as stderr:
            client = Client(args, stderr)
            report = smoke(args, client)
            client.close()
    finally:
        if client is not None and client.process.poll() is None:
            client.process.kill()
            client.process.wait(timeout=10)
        assert [digest(path) for path in source_paths] == before, "Read-only source changed"
    (args.project / "smoke-report.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()

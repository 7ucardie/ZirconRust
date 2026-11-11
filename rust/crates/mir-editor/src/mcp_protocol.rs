//! Sequential MCP stdio transport. Only JSON-RPC responses are written to stdout.
use serde_json::{json, Value};
use std::io::{BufRead, Read, Write};

pub const MAX_LINE_BYTES: usize = 1024 * 1024;
pub const PROTOCOL_VERSION: &str = "2025-11-25";
const WORLD_PROMPT: &str = "Create a playable Zircon map from the design brief below using the connected MCP tools. For a simple chamber, organic cave or 2–12-room Wood dungeon with one installed monster, call world_from_brief. For richer briefs, search installed monsters and maps before choosing IDs; never guess database IDs. Choose world_branch for Wood room graphs, world_perimeter for one to three enclosed Wood rooms, or world_generate for chamber, organic cave, or room dungeon. Use draft_variant only when the brief requests a new monster or item. Choose a fresh output revision, then inspect the generated map and report its map, database, and private assets paths. If the brief requests extra exact monster spawns, look up the registered map ID and use world_spawn on valid walkable cells. Keep all outputs in private project revisions. If a requested layout cannot be represented by these tools, explain the precise limit and produce the closest playable map only if the brief permits it. Design brief:\n\n";
const SPRITE_PROMPT: &str = "Design original editable Zircon sprite poses from this brief using the connected authoring tools and an image-generation tool if available. First export the matching source appearance with sprite_export_project, then inspect contract.json and reference/contact-sheet.png. Keep the contract's exact actions, directions, frame indices, scale, offsets and foot anchors. Draw new body poses for every required frame; use sprite_import_image or sprite_import_sheet with explicit slot mappings. A reference pose transfer only copies existing motion and does not satisfy a request for new motion. Validate with sprite_validate_project, build with sprite_build_pack, and visually inspect contact sheets plus native playback for direction errors, feet sliding, flicker and layer alignment. If a required frame cannot be drawn consistently, report the incomplete actions and do not publish. Use equipment_publish for complete weapon/helmet/shield families or wearable_publish for armour, always to private revisions. Sprite brief:\n\n";

fn error(id: &Value, code: i32, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
}
fn result(id: &Value, value: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"result":value})
}
fn send(writer: &mut impl Write, value: &Value) -> anyhow::Result<()> {
    serde_json::to_writer(&mut *writer, value)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

pub fn run<R: BufRead, W: Write>(
    mut reader: R,
    mut writer: W,
    tools: Vec<Value>,
    mut handler: impl FnMut(&str, Value) -> anyhow::Result<Value>,
) -> anyhow::Result<()> {
    let mut initialized = false;
    let mut ready = false;
    loop {
        let mut line = Vec::new();
        let count = (&mut reader)
            .take((MAX_LINE_BYTES + 2) as u64)
            .read_until(b'\n', &mut line)?;
        if count == 0 {
            return Ok(());
        }
        if line.last() == Some(&b'\n') {
            line.pop();
        }
        anyhow::ensure!(
            line.len() <= MAX_LINE_BYTES,
            "MCP input exceeds 1 MiB line limit."
        );
        let message: Value = match serde_json::from_slice(&line) {
            Ok(value) => value,
            Err(_) => {
                send(&mut writer, &error(&Value::Null, -32700, "Invalid JSON"))?;
                continue;
            }
        };
        let id = message.get("id");
        let method = message.get("method").and_then(Value::as_str);
        let valid_id = id.is_none_or(|id| id.is_string() || id.is_i64() || id.is_u64());
        if !message.is_object() || message["jsonrpc"] != "2.0" || method.is_none() || !valid_id {
            send(
                &mut writer,
                &error(
                    id.filter(|_| valid_id).unwrap_or(&Value::Null),
                    -32600,
                    "Invalid JSON-RPC request",
                ),
            )?;
            continue;
        }
        let method = method.unwrap();
        // Notifications never execute tools and never receive a response.
        let Some(id) = id else {
            if method == "notifications/initialized" && initialized {
                ready = true;
            }
            continue;
        };
        let params = message.get("params").cloned().unwrap_or_else(|| json!({}));
        let response = if !params.is_object() {
            error(id, -32602, "Parameters must be an object")
        } else {
            match method {
                "ping" => result(id, json!({})),
                "initialize" if initialized => error(id, -32600, "Already initialized"),
                "initialize" => {
                    if !params["protocolVersion"].is_string()
                        || !params["capabilities"].is_object()
                        || !params["clientInfo"]["name"].is_string()
                        || !params["clientInfo"]["version"].is_string()
                    {
                        error(
                            id,
                            -32602,
                            "Initialization requires protocolVersion, capabilities and clientInfo",
                        )
                    } else {
                        initialized = true;
                        result(
                            id,
                            json!({
                                "protocolVersion":PROTOCOL_VERSION,
                                "capabilities":{"tools":{"listChanged":false},"prompts":{"listChanged":false}},
                                "serverInfo":{"name":"zircon-editor","version":env!("CARGO_PKG_VERSION")}
                            }),
                        )
                    }
                }
                "tools/list" | "tools/call" | "prompts/list" | "prompts/get" if !ready => error(
                    id,
                    -32000,
                    "Complete initialize and notifications/initialized first",
                ),
                "tools/list" => {
                    if params.get("cursor").is_some() {
                        error(id, -32602, "This tool catalogue does not use cursors")
                    } else {
                        result(id, json!({"tools":tools}))
                    }
                }
                "prompts/list" => {
                    if params.get("cursor").is_some() {
                        error(id, -32602, "This prompt catalogue does not use cursors")
                    } else {
                        result(
                            id,
                            json!({"prompts":[{"name":"create_world","title":"Create ZirconRust map","description":"Turn a natural-language map brief into private playable map and content revisions using ZirconRust authoring tools.","arguments":[{"name":"request","description":"Describe the map, setting, rooms, monsters and items in ordinary language.","required":true}]},{"name":"design_sprite_motion","title":"Design original sprite motion","description":"Guide an AI host through drawing, importing and validating new game sprite poses.","arguments":[{"name":"request","description":"Describe the new sprite and motion in ordinary language.","required":true}]}]}),
                        )
                    }
                }
                "prompts/get" => {
                    let request = params["arguments"]["request"].as_str();
                    if params["name"] != "create_world" && params["name"] != "design_sprite_motion"
                    {
                        error(id, -32602, "Unknown prompt")
                    } else if let Some(request) =
                        request.filter(|text| !text.trim().is_empty() && text.len() <= 4000)
                    {
                        let sprite = params["name"] == "design_sprite_motion";
                        result(
                            id,
                            json!({"description":if sprite { "Design original game sprite motion from a natural-language brief." } else { "Create a playable Zircon map from a natural-language brief." },"messages":[{"role":"user","content":{"type":"text","text":format!("{}{request}",if sprite { SPRITE_PROMPT } else { WORLD_PROMPT })}}]}),
                        )
                    } else {
                        error(
                            id,
                            -32602,
                            "Prompt request must be 1–4000 nonblank characters",
                        )
                    }
                }
                "tools/call" => {
                    let name = params["name"].as_str();
                    let arguments = params
                        .get("arguments")
                        .cloned()
                        .unwrap_or_else(|| json!({}));
                    if let Some(name) = name.filter(|_| arguments.is_object()) {
                        if !tools.iter().any(|tool| tool["name"].as_str() == Some(name)) {
                            error(id, -32602, "Unknown tool")
                        } else {
                            let outcome = handler(name, arguments).and_then(|value| {
                                anyhow::ensure!(
                                    value.is_object(),
                                    "Tool returned non-object structured content"
                                );
                                Ok(value)
                            });
                            match outcome {
                                Ok(value) => result(
                                    id,
                                    json!({
                                        "content":[{"type":"text","text":serde_json::to_string(&value)?}],
                                        "structuredContent":value,"isError":false
                                    }),
                                ),
                                Err(failure) => result(
                                    id,
                                    json!({
                                        "content":[{"type":"text","text":format!("{failure:#}")}],"isError":true
                                    }),
                                ),
                            }
                        }
                    } else {
                        error(id, -32602, "Tool calls require a name and object arguments")
                    }
                }
                _ => error(id, -32601, "Unknown method"),
            }
        };
        send(&mut writer, &response)?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn initialize(version: &str) -> Value {
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
            "protocolVersion":version,"capabilities":{},"clientInfo":{"name":"test","version":"1"}
        }})
    }
    fn request(id: i32, method: &str, params: Value) -> Value {
        json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
    }
    fn exchange(messages: Vec<Value>) -> (Vec<Value>, usize) {
        let text = messages
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        let mut output = vec![];
        let mut calls = 0;
        run(
            Cursor::new(text),
            &mut output,
            vec![json!({"name":"echo","inputSchema":{"type":"object"}})],
            |_, arguments| {
                calls += 1;
                anyhow::ensure!(arguments.get("fail").is_none(), "Requested failure");
                Ok(arguments)
            },
        )
        .unwrap();
        (
            String::from_utf8(output)
                .unwrap()
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect(),
            calls,
        )
    }

    #[test]
    fn handshake_tools_and_structured_results() {
        let (responses, calls) = exchange(vec![
            initialize("2024-11-05"),
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
            request(2, "tools/list", json!({})),
            request(
                3,
                "tools/call",
                json!({"name":"echo","arguments":{"text":"two\nlines"}}),
            ),
            request(
                4,
                "tools/call",
                json!({"name":"echo","arguments":{"fail":true}}),
            ),
        ]);
        assert_eq!(responses.len(), 4);
        assert_eq!(responses[0]["result"]["protocolVersion"], PROTOCOL_VERSION);
        assert_eq!(responses[1]["result"]["tools"][0]["name"], "echo");
        assert_eq!(
            responses[2]["result"]["structuredContent"]["text"],
            "two\nlines"
        );
        assert_eq!(responses[3]["result"]["isError"], true);
        assert_eq!(calls, 2);
    }

    #[test]
    fn natural_language_world_prompt_is_discoverable_and_requires_a_brief() {
        let (responses, calls) = exchange(vec![
            initialize(PROTOCOL_VERSION),
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
            request(2, "prompts/list", json!({})),
            request(
                3,
                "prompts/get",
                json!({"name":"create_world","arguments":{"request":"A mossy Wood maze with six rooms and chickens"}}),
            ),
            request(
                4,
                "prompts/get",
                json!({"name":"create_world","arguments":{"request":"  "}}),
            ),
            request(
                5,
                "prompts/get",
                json!({"name":"design_sprite_motion","arguments":{"request":"New eight-direction sword stance"}}),
            ),
        ]);
        assert_eq!(
            responses[0]["result"]["capabilities"]["prompts"]["listChanged"],
            false
        );
        assert_eq!(responses[1]["result"]["prompts"][0]["name"], "create_world");
        assert_eq!(
            responses[1]["result"]["prompts"][1]["name"],
            "design_sprite_motion"
        );
        let brief = responses[2]["result"]["messages"][0]["content"]["text"]
            .as_str()
            .unwrap();
        assert!(brief.contains("six rooms and chickens"));
        assert!(brief.contains("world_branch"));
        assert_eq!(responses[3]["error"]["code"], -32602);
        let sprite = responses[4]["result"]["messages"][0]["content"]["text"]
            .as_str()
            .unwrap();
        assert!(sprite.contains("New eight-direction sword stance"));
        assert!(sprite.contains("sprite_import_sheet"));
        assert_eq!(calls, 0);
    }

    #[test]
    fn lifecycle_errors_unknown_calls_and_notifications_have_no_side_effects() {
        let (responses, calls) = exchange(vec![
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
            request(0, "ping", json!({})),
            request(2, "tools/list", json!({})),
            initialize(PROTOCOL_VERSION),
            request(3, "tools/call", json!({"name":"echo"})),
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
            json!({"jsonrpc":"2.0","method":"tools/call","params":{"name":"echo"}}),
            json!({"jsonrpc":"2.0","method":"unknown"}),
            request(4, "unknown", json!({})),
            request(5, "tools/call", json!({"name":"missing"})),
            request(6, "tools/call", json!({"name":"echo","arguments":[]})),
            initialize(PROTOCOL_VERSION),
        ]);
        assert_eq!(calls, 0);
        assert_eq!(responses.len(), 8);
        assert_eq!(responses[0]["result"], json!({}));
        for index in [1, 3] {
            assert_eq!(responses[index]["error"]["code"], -32000);
        }
        assert_eq!(responses[4]["error"]["code"], -32601);
        assert_eq!(responses[5]["error"]["code"], -32602);
        assert_eq!(responses[6]["error"]["code"], -32602);
        assert_eq!(responses[7]["error"]["code"], -32600);
    }

    #[test]
    fn malformed_input_recovers_but_oversized_lines_close_bounded_transport() {
        let mut output = vec![];
        run(
            Cursor::new(b"bad\n[]\n{\"jsonrpc\":\"2.0\",\"id\":null,\"method\":\"ping\"}\n"),
            &mut output,
            vec![],
            |_, _| unreachable!(),
        )
        .unwrap();
        let responses: Vec<Value> = String::from_utf8(output)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(responses[0]["error"]["code"], -32700);
        assert_eq!(responses[1]["error"]["code"], -32600);
        assert_eq!(responses[2]["error"]["code"], -32600);
        let mut input = Cursor::new(vec![b'x'; MAX_LINE_BYTES * 2]);
        let mut output = vec![];
        assert!(run(&mut input, &mut output, vec![], |_, _| unreachable!()).is_err());
        assert_eq!(input.position(), (MAX_LINE_BYTES + 2) as u64);
        assert!(output.is_empty());
    }

    #[test]
    fn invalid_initialization_does_not_advance_lifecycle_and_string_ids_are_preserved() {
        let (responses, calls) = exchange(vec![
            request(
                1,
                "initialize",
                json!({"protocolVersion": PROTOCOL_VERSION}),
            ),
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
            request(2, "tools/list", json!({})),
            json!({"jsonrpc":"2.0","id":"ping-id","method":"ping"}),
            request(3, "ping", json!([])),
            initialize(PROTOCOL_VERSION),
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
            request(4, "tools/call", json!({"name":"echo"})),
        ]);
        assert_eq!(responses[0]["error"]["code"], -32602);
        assert_eq!(responses[1]["error"]["code"], -32000);
        assert_eq!(responses[2]["id"], "ping-id");
        assert_eq!(responses[3]["error"]["code"], -32602);
        assert_eq!(responses[5]["result"]["structuredContent"], json!({}));
        assert_eq!(calls, 1);
    }
}

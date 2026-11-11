//! NPC page scripts (Zircon `NpcScriptEngine`, MoonSharp in the C#): a page
//! names a Lua file; its `Script` checks and actions call functions in it
//! with `(player, npc)` proxies, and `on_open` may redirect or cancel the
//! page. Scripts run in a sandbox (no io/os/package/debug) and their effects
//! on the world are recorded as commands the caller applies afterwards.
//!
//! Search order for `<file>`: `<data dir>/scripts/npc/`, `$ZIRCON_SCRIPTS`,
//! then this crate's `scripts/npc/` (examples). Files are cached; errors are
//! logged and make checks fail.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use mlua::{Lua, LuaOptions, MultiValue, StdLib, Table, Value};

/// What a script asked the world to do (applied after the call returns).
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    GiveGold(u64),
    TakeGold(u64),
    GiveItem(String, u32),
    TakeItem(String, u32),
    Message(String),
    /// `npc.navigate(page description)`; "" cancels the page.
    Navigate(String),
}

/// The player as the script sees it (`LuaPlayerProxy`).
#[derive(Debug, Clone, Default)]
pub struct PlayerView {
    pub name: String,
    pub level: i32,
    pub gold: u64,
    /// Lower-cased item name -> count in the bag.
    pub items: HashMap<String, u32>,
}

/// Result of a call: the function's return value and the recorded commands.
pub struct CallResult {
    pub value: Value,
    pub commands: Vec<Command>,
}

#[derive(Default)]
pub struct ScriptEngine {
    roots: Vec<PathBuf>,
    cache: HashMap<String, Option<Lua>>,
}

type Commands = Rc<RefCell<Vec<Command>>>;

impl ScriptEngine {
    pub fn new() -> ScriptEngine {
        let mut roots = Vec::new();
        if let Some(dir) = std::env::var_os("ZIRCON_SCRIPTS") {
            roots.push(PathBuf::from(dir));
        }
        roots.push(PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/scripts/npc"
        )));
        ScriptEngine {
            roots,
            cache: HashMap::new(),
        }
    }

    /// The data directory's `scripts/npc` takes precedence.
    pub fn set_root(&mut self, dir: PathBuf) {
        self.roots.insert(0, dir);
        self.cache.clear();
    }

    /// Forget every loaded script (reloads from disk on next use).
    pub fn invalidate(&mut self) {
        self.cache.clear();
    }

    fn find(&self, file: &str) -> Option<PathBuf> {
        let name = file.trim();
        if name.is_empty() || name.contains("..") {
            return None;
        }
        self.roots
            .iter()
            .map(|r| r.join(name))
            .find(|p| p.is_file())
    }

    fn script(&mut self, file: &str) -> Option<&Lua> {
        let key = file.to_ascii_lowercase();
        if !self.cache.contains_key(&key) {
            let loaded = self.find(file).and_then(|path| {
                let code = std::fs::read_to_string(&path).ok()?;
                let lua = match Lua::new_with(
                    StdLib::MATH | StdLib::STRING | StdLib::TABLE | StdLib::UTF8,
                    LuaOptions::default(),
                ) {
                    Ok(l) => l,
                    Err(e) => {
                        tracing::error!("[NpcScript] cannot create a Lua state: {e}");
                        return None;
                    }
                };
                if let Err(e) = lua.load(&code).set_name(file).exec() {
                    tracing::error!("[NpcScript] failed to load {file}: {e}");
                    return None;
                }
                Some(lua)
            });
            if loaded.is_none() {
                tracing::warn!("[NpcScript] script not usable: {file}");
            }
            self.cache.insert(key.clone(), loaded);
        }
        self.cache.get(&key).and_then(|l| l.as_ref())
    }

    /// Does `file` define a global function `name`?
    pub fn has_function(&mut self, file: &str, name: &str) -> bool {
        self.script(file)
            .and_then(|lua| lua.globals().get::<Value>(name).ok())
            .is_some_and(|v| matches!(v, Value::Function(_)))
    }

    /// Call `name(player, npc)`; `None` when the script or function is
    /// missing (Zircon treats that as "no opinion"), `Some(Err)` on a Lua
    /// error.
    pub fn call(
        &mut self,
        file: &str,
        name: &str,
        view: &PlayerView,
    ) -> Option<Result<CallResult, String>> {
        let lua = self.script(file)?;
        let func = match lua.globals().get::<Value>(name) {
            Ok(Value::Function(f)) => f,
            _ => return None,
        };
        let commands: Commands = Rc::new(RefCell::new(Vec::new()));
        let built = build_proxies(lua, view, &commands);
        let (player, npc) = match built {
            Ok(t) => t,
            Err(e) => return Some(Err(format!("{name} in {file}: {e}"))),
        };
        let result = func.call::<Value>((player, npc));
        Some(match result {
            Ok(value) => Ok(CallResult {
                value,
                commands: std::mem::take(&mut *commands.borrow_mut()),
            }),
            Err(e) => Err(format!("{name} in {file}: {e}")),
        })
    }
}

/// The `player` and `npc` tables handed to every script function.
fn build_proxies(
    lua: &Lua,
    view: &PlayerView,
    commands: &Commands,
) -> mlua::Result<(Table, Table)> {
    let player = lua.create_table()?;
    player.set("name", view.name.clone())?;
    player.set("level", view.level)?;
    player.set("gold", view.gold)?;
    let items = view.items.clone();
    player.set(
        "has_item",
        lua.create_function(move |_, args: MultiValue| {
            let (name, count) = name_count(&args);
            if count == 0 {
                return Ok(true);
            }
            Ok(items.get(&name.to_ascii_lowercase()).copied().unwrap_or(0) >= count)
        })?,
    )?;
    let npc = lua.create_table()?;
    let item_cmd = |f: fn(String, u32) -> Command| {
        let cmds = commands.clone();
        move |_: &Lua, args: MultiValue| {
            let (name, count) = name_count(&args);
            if count > 0 && !name.is_empty() {
                cmds.borrow_mut().push(f(name, count));
            }
            Ok(())
        }
    };
    npc.set(
        "give_item",
        lua.create_function(item_cmd(Command::GiveItem))?,
    )?;
    npc.set(
        "take_item",
        lua.create_function(item_cmd(Command::TakeItem))?,
    )?;
    let gold_cmd = |f: fn(u64) -> Command| {
        let cmds = commands.clone();
        move |_: &Lua, args: MultiValue| {
            let amount = first_number(&args).max(0.0) as u64;
            if amount > 0 {
                cmds.borrow_mut().push(f(amount));
            }
            Ok(())
        }
    };
    npc.set(
        "give_gold",
        lua.create_function(gold_cmd(Command::GiveGold))?,
    )?;
    npc.set(
        "take_gold",
        lua.create_function(gold_cmd(Command::TakeGold))?,
    )?;
    let text_cmd = |f: fn(String) -> Command| {
        let cmds = commands.clone();
        move |_: &Lua, args: MultiValue| {
            cmds.borrow_mut().push(f(first_string(&args)));
            Ok(())
        }
    };
    npc.set("message", lua.create_function(text_cmd(Command::Message))?)?;
    npc.set(
        "navigate",
        lua.create_function(text_cmd(Command::Navigate))?,
    )?;
    Ok((player, npc))
}

/// `(name, count)` from `f(name)`, `f(name, count)` or `obj:f(name, count)`.
fn name_count(args: &MultiValue) -> (String, u32) {
    let mut it = args.iter().skip_while(|v| matches!(v, Value::Table(_)));
    let name = match it.next() {
        Some(Value::String(s)) => s.to_str().map(|s| s.to_string()).unwrap_or_default(),
        _ => String::new(),
    };
    let count = match it.next() {
        Some(Value::Integer(i)) => (*i).max(0) as u32,
        Some(Value::Number(n)) => n.max(0.0) as u32,
        _ => 1,
    };
    (name, count)
}

fn first_number(args: &MultiValue) -> f64 {
    args.iter()
        .skip_while(|v| matches!(v, Value::Table(_)))
        .find_map(|v| match v {
            Value::Integer(i) => Some(*i as f64),
            Value::Number(n) => Some(*n),
            _ => None,
        })
        .unwrap_or(0.0)
}

fn first_string(args: &MultiValue) -> String {
    args.iter()
        .skip_while(|v| matches!(v, Value::Table(_)))
        .find_map(|v| match v {
            Value::String(s) => s.to_str().ok().map(|s| s.to_string()),
            Value::Nil => Some(String::new()),
            _ => None,
        })
        .unwrap_or_default()
}

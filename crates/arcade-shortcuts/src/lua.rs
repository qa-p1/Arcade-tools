//! Evaluate Hyprland Lua with no process execution, filesystem writes or network.
//!
//! A fresh VM owns each evaluation. Only selected standard libraries and explicit
//! read-only replacements are installed; `require` never calls system loaders.

mod io;

use crate::{keys, labels, read, Action, ConfigBind, ConfigReport, Origin, ReadPolicy};
use chrono::{Datelike, TimeZone, Timelike};
use mlua::chunk::ChunkMode;
use mlua::{HookTriggers, Lua, LuaOptions, MultiValue, StdLib, Table, Value, VmState};
use serde_json::{Map, Value as Json};
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

const DISABLED: &str = "disabled in Arcade's sandbox";
const MAX_SOURCE_BYTES: usize = 8 * 1024 * 1024;
const MAX_BINDS: usize = 10_000;
const MAX_RECORDED_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct SandboxOptions {
    pub config_dir: PathBuf,
    pub read_policy: ReadPolicy,
    /// Per-evaluation overrides; `None` hides an environment variable. This
    /// never mutates the process environment (particularly HOME in tests).
    pub environment: BTreeMap<String, Option<String>>,
    pub memory_limit: usize,
    pub instruction_limit: u64,
    pub duration_limit: Duration,
}

impl SandboxOptions {
    pub fn new(config_dir: impl Into<PathBuf>) -> Self {
        Self {
            config_dir: config_dir.into(),
            read_policy: ReadPolicy::user_config(),
            environment: BTreeMap::new(),
            memory_limit: 64 * 1024 * 1024,
            instruction_limit: 50_000_000,
            duration_limit: Duration::from_millis(100),
        }
    }

    pub fn getenv(&self, name: &str) -> Option<String> {
        self.environment
            .get(name)
            .cloned()
            .unwrap_or_else(|| std::env::var(name).ok())
    }
}

#[derive(Debug)]
struct LimitExceeded(&'static str);

fn abort(reason: &'static str) -> ! {
    // Unlike a Lua error, this cannot be swallowed by pcall/xpcall. resume_unwind
    // avoids invoking the process panic hook for an expected sandbox limit.
    std::panic::resume_unwind(Box::new(LimitExceeded(reason)))
}

pub fn evaluate(path: &Path, options: &SandboxOptions) -> ConfigReport {
    let report = Rc::new(RefCell::new(ConfigReport::default()));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run(path, options, report.clone())
    }));
    match result {
        Ok(Ok(())) => {}
        Ok(Err(e)) => report
            .borrow_mut()
            .warnings
            .push(format!("Lua evaluation: {e}")),
        Err(payload) => match payload.downcast::<LimitExceeded>() {
            Ok(reason) => report
                .borrow_mut()
                .warnings
                .push(format!("Lua evaluation aborted: {}", reason.0)),
            Err(payload) => std::panic::resume_unwind(payload),
        },
    }
    let result = std::mem::take(&mut *report.borrow_mut());
    result
}

fn run(
    path: &Path,
    options: &SandboxOptions,
    report: Rc<RefCell<ConfigReport>>,
) -> mlua::Result<()> {
    let (path, source) = options
        .read_policy
        .read_text(path)
        .map_err(mlua::Error::external)?;
    report
        .borrow_mut()
        .sources
        .insert(path.clone(), source.clone());
    let lua = Lua::new_with(
        StdLib::STRING | StdLib::TABLE | StdLib::MATH | StdLib::UTF8,
        LuaOptions::new().catch_rust_panics(false),
    )?;
    lua.set_memory_limit(options.memory_limit)?;
    let start = Instant::now();
    let instructions = Cell::new(0u64);
    let instruction_limit = options.instruction_limit;
    let duration_limit = options.duration_limit;
    let step = instruction_limit.clamp(1, 1000) as u32;
    lua.set_hook(
        HookTriggers::new().every_nth_instruction(step),
        move |_, _| {
            let n = instructions.get().saturating_add(step as u64);
            instructions.set(n);
            if n > instruction_limit {
                abort("instruction limit exceeded");
            }
            if start.elapsed() > duration_limit {
                abort("time limit exceeded");
            }
            Ok(VmState::Continue)
        },
    )?;

    let env = lua.globals();
    for name in [
        "dofile",
        "loadfile",
        "debug",
        "package",
        "io",
        "os",
        "coroutine",
    ] {
        env.set(name, Value::Nil)?;
    }
    env.set("print", lua.create_function(|_, _: MultiValue| Ok(()))?)?;
    env.set("warn", lua.create_function(|_, _: MultiValue| Ok(()))?)?;
    env.set("os", os_table(&lua, options.clone(), start)?)?;
    env.set(
        "io",
        io::install(
            &lua,
            options.read_policy.clone(),
            options.getenv("HOME").map(PathBuf::from),
        )?,
    )?;
    env.set(
        "load",
        lua.create_function(
            |lua,
             (source, name, _mode, _env): (
                mlua::LuaString,
                Option<String>,
                Option<String>,
                Option<Table>,
            )| {
                let source = source.as_bytes();
                match lua
                    .load(&*source)
                    .set_name(name.unwrap_or_else(|| "=sandbox load".into()))
                    .set_mode(ChunkMode::Text)
                    .set_environment(lua.globals())
                    .into_function()
                {
                    Ok(function) => Ok((Value::Function(function), Value::Nil)),
                    Err(e) => Ok((Value::Nil, Value::String(lua.create_string(e.to_string())?))),
                }
            },
        )?,
    )?;
    install_require(&lua, options, report.clone())?;
    install_hl(&lua, report)?;
    lua.load(&source)
        .set_name(format!("@{}", path.display()))
        .set_mode(ChunkMode::Text)
        .set_environment(env)
        .exec()
}

fn os_table(lua: &Lua, options: SandboxOptions, start: Instant) -> mlua::Result<Table> {
    let os = lua.create_table()?;
    os.set(
        "getenv",
        lua.create_function(move |_, name: String| Ok(options.getenv(&name)))?,
    )?;
    os.set(
        "clock",
        lua.create_function(move |_, ()| Ok(start.elapsed().as_secs_f64()))?,
    )?;
    os.set(
        "difftime",
        lua.create_function(|_, (a, b): (f64, f64)| Ok(a - b))?,
    )?;
    os.set(
        "time",
        lua.create_function(|_, table: Option<Table>| {
            if let Some(t) = table {
                let year: i32 = t.get("year")?;
                let month: u32 = t.get("month")?;
                let day: u32 = t.get("day")?;
                let hour = t.get::<Option<u32>>("hour")?.unwrap_or(12);
                let minute = t.get::<Option<u32>>("min")?.unwrap_or(0);
                let second = t.get::<Option<u32>>("sec")?.unwrap_or(0);
                let date = chrono::NaiveDate::from_ymd_opt(year, month, day)
                    .and_then(|d| d.and_hms_opt(hour, minute, second))
                    .ok_or_else(|| mlua::Error::runtime("invalid date"))?;
                Ok(chrono::Local
                    .from_local_datetime(&date)
                    .earliest()
                    .ok_or_else(|| mlua::Error::runtime("invalid local date"))?
                    .timestamp())
            } else {
                Ok(chrono::Utc::now().timestamp())
            }
        })?,
    )?;
    os.set(
        "date",
        lua.create_function(|lua, (format, time): (Option<String>, Option<i64>)| {
            let format = format.as_deref().unwrap_or("%c");
            let dt = chrono::DateTime::from_timestamp(
                time.unwrap_or_else(|| chrono::Utc::now().timestamp()),
                0,
            )
            .ok_or_else(|| mlua::Error::runtime("invalid timestamp"))?;
            let date = if format.starts_with('!') {
                dt.fixed_offset()
            } else {
                dt.with_timezone(&chrono::Local).fixed_offset()
            };
            let format = format.trim_start_matches('!');
            if format == "*t" {
                let t = lua.create_table()?;
                for (key, value) in [
                    ("year", date.year() as i64),
                    ("month", date.month() as i64),
                    ("day", date.day() as i64),
                    ("hour", date.hour() as i64),
                    ("min", date.minute() as i64),
                    ("sec", date.second() as i64),
                    ("wday", date.weekday().number_from_sunday() as i64),
                    ("yday", date.ordinal() as i64),
                ] {
                    t.set(key, value)?;
                }
                t.set("isdst", false)?;
                Ok(Value::Table(t))
            } else {
                use std::fmt::Write;
                let mut text = String::new();
                write!(&mut text, "{}", date.format(format))
                    .map_err(|_| mlua::Error::runtime("invalid date format"))?;
                Ok(Value::String(lua.create_string(text)?))
            }
        })?,
    )?;
    for name in [
        "execute",
        "remove",
        "rename",
        "exit",
        "tmpname",
        "setlocale",
    ] {
        os.set(
            name,
            lua.create_function(|_, _: MultiValue| Ok((Option::<bool>::None, DISABLED)))?,
        )?;
    }
    Ok(os)
}

fn install_require(
    lua: &Lua,
    options: &SandboxOptions,
    report: Rc<RefCell<ConfigReport>>,
) -> mlua::Result<()> {
    let package = lua.create_table()?;
    package.set(
        "path",
        format!(
            "{}/?.lua;{}/?/init.lua",
            options.config_dir.display(),
            options.config_dir.display()
        ),
    )?;
    let loaded = lua.create_table()?;
    package.set("loaded", loaded.clone())?;
    lua.globals().set("package", package.clone())?;
    let policy = options.read_policy.clone();
    let home = options.getenv("HOME").map(PathBuf::from);
    let depth = Cell::new(0usize);
    lua.globals().set(
        "require",
        lua.create_function(move |lua, name: String| {
            let cached: Value = loaded.raw_get(name.as_str())?;
            if !matches!(cached, Value::Nil | Value::Boolean(false)) {
                return Ok(cached);
            }
            if depth.get() >= 32 {
                abort("module depth limit exceeded");
            }
            // Cache before executing to tolerate cyclic imports.
            let empty = lua.create_table()?;
            loaded.raw_set(name.as_str(), empty.clone())?;
            let module = name.replace('.', "/");
            let path: String = package.get("path")?;
            let mut found = None;
            let mut rejection = None;
            for pattern in path.split(';').take(128) {
                let path = read::expand_home(&pattern.replace('?', &module), home.as_deref());
                match policy.read_text(&path) {
                    Ok(file) => {
                        found = Some(file);
                        break;
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => rejection = Some(e.to_string()),
                }
            }
            let Some((path, source)) = found else {
                let reason = rejection.unwrap_or_else(|| "module not found".into());
                warning(
                    &report,
                    format!("require({name:?}): {reason}; using an empty table"),
                );
                return Ok(Value::Table(empty));
            };
            {
                let mut report = report.borrow_mut();
                let total: usize = report.sources.values().map(String::len).sum();
                if total.saturating_add(source.len()) > MAX_SOURCE_BYTES {
                    abort("source size limit exceeded");
                }
                report.sources.insert(path.clone(), source.clone());
            }
            depth.set(depth.get() + 1);
            let result = lua
                .load(&source)
                .set_name(format!("@{}", path.display()))
                .set_mode(ChunkMode::Text)
                .set_environment(lua.globals())
                .eval::<Value>();
            depth.set(depth.get() - 1);
            let value = match result {
                Ok(Value::Nil) => Value::Boolean(true),
                Ok(value) => value,
                Err(e) => {
                    warning(
                        &report,
                        format!(
                            "require({name:?}) in {}: {e}; using an empty table",
                            path.display()
                        ),
                    );
                    Value::Table(empty)
                }
            };
            loaded.raw_set(name, value.clone())?;
            Ok(value)
        })?,
    )?;
    Ok(())
}

fn warning(report: &Rc<RefCell<ConfigReport>>, text: String) {
    let mut report = report.borrow_mut();
    if report.warnings.len() < 128 {
        report.warnings.push(text);
    }
}

fn sink(lua: &Lua) -> mlua::Result<Table> {
    let table = lua.create_table()?;
    let meta = lua.create_table()?;
    let indexed = table.clone();
    meta.set(
        "__index",
        lua.create_function(move |_, _: MultiValue| Ok(indexed.clone()))?,
    )?;
    let called = table.clone();
    meta.set(
        "__call",
        lua.create_function(move |_, _: MultiValue| Ok(called.clone()))?,
    )?;
    meta.set(
        "__newindex",
        lua.create_function(|_, _: MultiValue| Ok(()))?,
    )?;
    meta.set("__metatable", "Arcade sandbox sink")?;
    table.set_metatable(Some(meta))?;
    Ok(table)
}

fn descriptor(lua: &Lua, path: String) -> mlua::Result<Table> {
    let table = lua.create_table()?;
    let meta = lua.create_table()?;
    let parent = path.clone();
    meta.set(
        "__index",
        lua.create_function(move |lua, (_, key): (Table, String)| {
            let name = if parent.is_empty() {
                key
            } else {
                format!("{parent}.{key}")
            };
            descriptor(lua, name)
        })?,
    )?;
    meta.set(
        "__call",
        lua.create_function(move |lua, (_, args): (Table, MultiValue)| {
            let marker = lua.create_table()?;
            marker.set("__arcade_dispatcher", true)?;
            marker.set("name", path.clone())?;
            let values = lua.create_table()?;
            marker.set("arg_count", args.len())?;
            for (i, value) in args.into_iter().enumerate() {
                values.raw_set(i + 1, value)?;
            }
            marker.set("args", values)?;
            Ok(marker)
        })?,
    )?;
    table.set_metatable(Some(meta))?;
    Ok(table)
}

fn install_hl(lua: &Lua, report: Rc<RefCell<ConfigReport>>) -> mlua::Result<()> {
    let hl = lua.create_table()?;
    hl.set("dsp", descriptor(lua, String::new())?)?;
    let current_submap = Rc::new(RefCell::new(None::<String>));
    let bind_submap = current_submap.clone();
    let recorded_bytes = Cell::new(0usize);
    hl.set(
        "bind",
        lua.create_function(
            move |lua, (key, action, opts): (String, Value, Option<Table>)| {
                if report.borrow().binds.len() >= MAX_BINDS {
                    abort("bind count limit exceeded");
                }
                if key.len() > 1024 {
                    abort("key string size limit exceeded");
                }
                let origin = call_site(lua, &report.borrow().sources);
                let action = match action {
                    Value::Function(function) => {
                        let info = function.info();
                        Action::LuaFunction {
                            file: info
                                .source
                                .and_then(|s| s.strip_prefix('@').map(PathBuf::from)),
                            line: info.line_defined.and_then(|n| u32::try_from(n).ok()),
                        }
                    }
                    Value::Table(table)
                        if table
                            .raw_get::<bool>("__arcade_dispatcher")
                            .unwrap_or(false) =>
                    {
                        let name = table.raw_get("name")?;
                        let args: Table = table.raw_get("args")?;
                        let count = table.raw_get::<usize>("arg_count")?.min(128);
                        let mut budget = JsonBudget {
                            nodes: 2048,
                            bytes: 128 * 1024,
                        };
                        let args = (1..=count)
                            .map(|i| json_value(args.raw_get(i)?, 0, &mut budget))
                            .collect::<mlua::Result<_>>()?;
                        Action::Dispatcher { name, args }
                    }
                    _ => Action::Unknown,
                };
                let mut flags = BTreeMap::new();
                let mut description = None;
                let mut submap = bind_submap.borrow().clone();
                if let Some(opts) = opts {
                    for item in opts.pairs::<Value, Value>().take(128) {
                        let (key, value) = item?;
                        if let Value::String(key) = key {
                            let key = key.to_string_lossy();
                            match (key.as_str(), value) {
                                ("desc", Value::String(s)) => {
                                    if s.as_bytes().len() > 4096 {
                                        abort("description size limit exceeded");
                                    }
                                    description = Some(s.to_string_lossy())
                                }
                                ("submap", Value::String(s)) => {
                                    if s.as_bytes().len() > 1024 {
                                        abort("submap size limit exceeded");
                                    }
                                    submap = read::submap(&s.to_string_lossy())
                                }
                                (_, Value::Boolean(value)) => {
                                    if key.len() > 256 {
                                        continue;
                                    }
                                    flags.insert(key, value);
                                }
                                _ => {}
                            }
                        }
                    }
                }
                let bytes = key.len()
                    + description.as_ref().map_or(0, String::len)
                    + serde_json::to_vec(&action)
                        .map_err(mlua::Error::external)?
                        .len()
                    + flags.len() * 256;
                let total = recorded_bytes.get().saturating_add(bytes);
                if total > MAX_RECORDED_BYTES {
                    abort("recorded bind memory limit exceeded");
                }
                recorded_bytes.set(total);
                report.borrow_mut().binds.push(ConfigBind {
                    chord: keys::chord_from_key_string(&key),
                    key_text: key,
                    submap,
                    action,
                    flags,
                    description,
                    origin,
                });
                sink(lua)
            },
        )?,
    )?;
    hl.set(
        "define_submap",
        lua.create_function(move |lua, (name, args): (Value, MultiValue)| {
            let name = match name {
                Value::String(s) => read::submap(&s.to_string_lossy()),
                Value::Table(t) => t
                    .raw_get::<Option<String>>("name")
                    .ok()
                    .flatten()
                    .and_then(|s| read::submap(&s)),
                _ => None,
            };
            let old = current_submap.replace(name);
            let function = args.into_iter().find_map(|v| match v {
                Value::Function(f) => Some(f),
                _ => None,
            });
            let result = function.map_or(Ok(()), |f| f.call::<()>(()));
            current_submap.replace(old);
            result?;
            sink(lua)
        })?,
    )?;
    let meta = lua.create_table()?;
    meta.set(
        "__index",
        lua.create_function(|lua, (_, key): (Table, Value)| {
            if let Value::String(key) = key {
                if key.to_str()?.starts_with("get_") {
                    return Ok(Value::Function(
                        lua.create_function(|_, _: MultiValue| Ok(Value::Nil))?,
                    ));
                }
            }
            Ok(Value::Table(sink(lua)?))
        })?,
    )?;
    hl.set_metatable(Some(meta))?;
    lua.globals().set("hl", hl)
}

fn call_site(lua: &Lua, sources: &BTreeMap<PathBuf, String>) -> Option<Origin> {
    for level in 0..128 {
        let frame = lua.inspect_stack(level, |frame| {
            let source = frame.source();
            if source.what != "main" {
                return None;
            }
            let path = PathBuf::from(source.source?.strip_prefix('@')?);
            let text = sources.get(&path)?;
            let line = u32::try_from(frame.current_line()?).ok()?;
            Some(Origin {
                file: path,
                line: labels::statement_start(text, line),
            })
        });
        match frame {
            Some(Some(origin)) => return Some(origin),
            Some(None) => {}
            None => break,
        }
    }
    None
}

/// No serde hooks/metamethods: arguments are data, never executed.
struct JsonBudget {
    nodes: usize,
    bytes: usize,
}

fn json_value(value: Value, depth: usize, budget: &mut JsonBudget) -> mlua::Result<Json> {
    if depth > 8 || budget.nodes == 0 || budget.bytes == 0 {
        return Ok(Json::String("<limit>".into()));
    }
    budget.nodes -= 1;
    Ok(match value {
        Value::Nil => Json::Null,
        Value::Boolean(b) => Json::Bool(b),
        Value::Integer(n) => Json::from(n),
        Value::Number(n) => Json::from(n),
        Value::String(s) => {
            let text: String = s
                .to_string_lossy()
                .chars()
                .take(budget.bytes.min(4096) / 4)
                .collect();
            budget.bytes = budget.bytes.saturating_sub(text.len());
            Json::String(text)
        }
        Value::Function(_) => Json::String("<function>".into()),
        Value::Table(t) => {
            let entries = t
                .pairs::<Value, Value>()
                .take(256)
                .collect::<mlua::Result<Vec<_>>>()?;
            let is_array = !entries.is_empty()
                && entries.iter().all(
                    |(k, _)| matches!(k, Value::Integer(n) if *n > 0 && *n <= entries.len() as i64),
                );
            if is_array {
                let mut values = vec![Json::Null; entries.len()];
                for (key, value) in entries {
                    if let Value::Integer(n) = key {
                        values[n as usize - 1] = json_value(value, depth + 1, budget)?;
                    }
                }
                Json::Array(values)
            } else {
                let mut object = Map::new();
                for (key, value) in entries {
                    let key = match key {
                        Value::String(s) => s.to_string_lossy().chars().take(256).collect(),
                        Value::Integer(n) => n.to_string(),
                        _ => continue,
                    };
                    object.insert(key, json_value(value, depth + 1, budget)?);
                }
                Json::Object(object)
            }
        }
        _ => Json::String("<userdata>".into()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_script(source: &str, setup: impl FnOnce(&Path, &mut SandboxOptions)) -> ConfigReport {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("hyprland.lua");
        std::fs::write(&file, source).unwrap();
        let mut opts = SandboxOptions::new(dir.path());
        opts.read_policy = ReadPolicy::new([dir.path().to_owned()]);
        setup(dir.path(), &mut opts);
        evaluate(&file, &opts)
    }

    #[test]
    fn captures_helpers_descriptors_flags_submaps_and_unknown_api() {
        let report = run_script("local function helper(k, a) hl.bind(k,a,{desc='Hi',locked=true}) end\nhelper(\n 'SUPER + T',\n hl.dsp.window.move({workspace=3,nested={1,2},cb=function()end})\n)\nhl.unknown.field(42):method().anything=3\nhl.monitor({}):set_enabled(true)\nassert(hl.get_active_window()==nil)\nhl.define_submap('resize', function() hl.bind('R',hl.dsp.no_op()) end)\nhl.define_submap('other', {})\nhl.define_submap({name='table-form'})\nhl.define_submap(nil)\nhl.bind('Q',function() end,{submap='explicit'})", |_, _| {});
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        assert_eq!(report.binds.len(), 3);
        assert_eq!(report.binds[0].origin.as_ref().unwrap().line, 2);
        assert_eq!(report.binds[0].description.as_deref(), Some("Hi"));
        assert_eq!(report.binds[0].flags.get("locked"), Some(&true));
        assert_eq!(report.binds[1].submap.as_deref(), Some("resize"));
        assert_eq!(report.binds[2].submap.as_deref(), Some("explicit"));
        let Action::Dispatcher { args, .. } = &report.binds[0].action else {
            panic!()
        };
        assert_eq!(args[0]["nested"], serde_json::json!([1, 2]));
        assert_eq!(args[0]["cb"], "<function>");
    }

    #[test]
    fn require_recovers_missing_and_erroring_modules_and_keeps_partial_binds() {
        let report = run_script("require('missing'); require('broken'); require('broken'); hl.bind('B',hl.dsp.exit()); error('top error')", |dir, _| {
            std::fs::write(dir.join("broken.lua"), "hl.bind('A',hl.dsp.no_op()); error('broken error')").unwrap();
        });
        assert_eq!(report.binds.len(), 2);
        assert_eq!(report.warnings.len(), 3);
        assert!(report.warnings[0].contains("missing"));
        assert!(report.warnings[1].contains("broken error"));
        assert!(report.warnings[2].contains("top error"));
    }

    #[test]
    fn process_writes_and_binary_loaders_are_disabled() {
        let report = run_script("assert(debug==nil and dofile==nil and loadfile==nil and package.loadlib==nil); local a,b=os.execute('touch forbidden'); assert(a==nil and b:find('disabled')); assert(io.popen('touch forbidden')==nil); assert(os.remove('keep')==nil); assert(os.rename('keep','gone')==nil); assert(os.exit()==nil); assert(os.tmpname()==nil); local f=io.open('forbidden','w'); f:write('bad'); f:close(); local f=io.open('keep','a'); f:write('bad'); f:close(); assert(load(string.char(27)..'Lua')==nil); local f=load('return io.popen, os.execute',nil,'b',{os={execute=print}}); assert(type(f)=='function'); assert(os.date('!%Y',0)=='1970'); assert(type(os.date('!*t',0))=='table'); hl.bind('A',hl.dsp.exit())", |dir, opts| {
            std::fs::write(dir.join("keep"), "keep").unwrap();
            opts.environment.insert("HOME".into(), Some(dir.to_string_lossy().into_owned()));
        });
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        assert_eq!(report.binds.len(), 1);
        // Fake writes never open their paths, including relative/outside paths.
        assert!(!Path::new("forbidden").exists());
    }

    #[test]
    fn instruction_and_memory_limits_preserve_partial_results() {
        let report = run_script("hl.bind('A',hl.dsp.no_op()); while true do pcall(function() while true do end end) end", |_, opts| {
            opts.instruction_limit = 10_000;
        });
        assert_eq!(report.binds.len(), 1);
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.contains("instruction limit")),
            "{:?}",
            report.warnings
        );
        let report = run_script(
            "hl.bind('A',hl.dsp.no_op()); local x=string.rep('x',10000000)",
            |_, opts| {
                opts.memory_limit = 1024 * 1024;
            },
        );
        assert_eq!(report.binds.len(), 1);
        assert!(
            report.warnings.iter().any(|w| w.contains("memory")),
            "{:?}",
            report.warnings
        );
    }

    #[test]
    fn rust_side_reads_and_descriptors_are_bounded() {
        let report = run_script("hl.bind('A',hl.dsp.no_op()); local files={}; for i=1,4 do files[i]=io.open(os.getenv('HOME')..'/data','r') end", |dir, opts| {
            std::fs::write(dir.join("data"), vec![b'x'; 3 * 1024 * 1024]).unwrap();
            opts.environment.insert("HOME".into(), Some(dir.to_string_lossy().into_owned()));
        });
        assert_eq!(report.binds.len(), 1);
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.contains("file read memory limit")),
            "{:?}",
            report.warnings
        );
        let report = run_script("local t={}; t.self=t; hl.bind('A',hl.dsp.new_api(nil,t,function()end)); hl.bind('B',function()end)", |_, _| {});
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        let Action::Dispatcher { args, .. } = &report.binds[0].action else {
            panic!()
        };
        assert_eq!(args[0], Json::Null);
        assert_eq!(args[2], "<function>");
        assert!(args[1].to_string().contains("<limit>"));
        let Action::LuaFunction { file, line } = &report.binds[1].action else {
            panic!()
        };
        assert!(file.as_ref().unwrap().ends_with("hyprland.lua"));
        assert_eq!(*line, Some(1));
    }

    #[cfg(unix)]
    #[test]
    fn both_io_and_require_reject_symlink_escapes() {
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(
            outside.path().join("escape.lua"),
            "hl.bind('B',hl.dsp.exit())",
        )
        .unwrap();
        let report = run_script("assert(io.open(os.getenv('HOME')..'/escape.lua')==nil); require('escape'); hl.bind('A',hl.dsp.exit())", |dir, opts| {
            std::os::unix::fs::symlink(outside.path().join("escape.lua"), dir.join("escape.lua")).unwrap();
            opts.environment.insert("HOME".into(), Some(dir.to_string_lossy().into_owned()));
        });
        assert_eq!(report.binds.len(), 1);
        assert!(report
            .warnings
            .iter()
            .any(|w| w.contains("escapes sandbox")));
    }
}

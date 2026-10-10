//! In-memory read handles and discard-only writable handles. No OS writes.

use crate::read::{expand_home, ReadPolicy};
use mlua::{Function, Lua, MultiValue, Table, UserData, UserDataMethods, Value};
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

const DISABLED: &str = "disabled in Arcade's sandbox";

#[derive(Default)]
struct FileData {
    bytes: Vec<u8>,
    pos: usize,
    closed: bool,
}

#[derive(Clone)]
struct SandboxFile(Rc<RefCell<FileData>>);

impl SandboxFile {
    fn new(bytes: Vec<u8>) -> Self {
        Self(Rc::new(RefCell::new(FileData {
            bytes,
            ..FileData::default()
        })))
    }

    fn read(&self, lua: &Lua, formats: MultiValue) -> mlua::Result<MultiValue> {
        let mut file = self.0.borrow_mut();
        if file.closed {
            return Err(mlua::Error::runtime("attempt to read a closed file"));
        }
        let formats = if formats.is_empty() {
            MultiValue::from_vec(vec![Value::String(lua.create_string("l")?)])
        } else {
            formats
        };
        let mut values = MultiValue::new();
        for format in formats {
            let start = file.pos.min(file.bytes.len());
            let value = match format {
                Value::Integer(n) if n >= 0 => {
                    let n = (n as u64).min(file.bytes.len() as u64) as usize;
                    if start == file.bytes.len() {
                        Value::Nil
                    } else {
                        let end = start.saturating_add(n).min(file.bytes.len());
                        let value = Value::String(lua.create_string(&file.bytes[start..end])?);
                        file.pos = end;
                        value
                    }
                }
                Value::String(s) => match s.to_str()?.trim_start_matches('*') {
                    "a" => {
                        let value = Value::String(lua.create_string(&file.bytes[start..])?);
                        file.pos = file.bytes.len();
                        value
                    }
                    "l" | "L" => {
                        if start == file.bytes.len() {
                            Value::Nil
                        } else {
                            let newline = file.bytes[start..]
                                .iter()
                                .position(|b| *b == b'\n')
                                .map(|p| start + p);
                            let end = newline.unwrap_or(file.bytes.len());
                            let keep = s.to_str()?.ends_with('L');
                            let output_end = if keep {
                                newline.map_or(end, |n| n + 1)
                            } else {
                                end
                            };
                            let trim_end = if !keep
                                && output_end > start
                                && file.bytes.get(output_end.wrapping_sub(1)) == Some(&b'\r')
                            {
                                output_end - 1
                            } else {
                                output_end
                            };
                            let value =
                                Value::String(lua.create_string(&file.bytes[start..trim_end])?);
                            file.pos = newline.map_or(end, |n| n + 1);
                            value
                        }
                    }
                    "n" => {
                        file.pos = start;
                        while file.pos < file.bytes.len()
                            && file.bytes[file.pos].is_ascii_whitespace()
                        {
                            file.pos += 1;
                        }
                        let start = file.pos;
                        let end = file.bytes[start..]
                            .iter()
                            .position(|b| !b.is_ascii_digit() && !b"+-.eExXaAbBcCdDfF".contains(b))
                            .map_or(file.bytes.len(), |p| start + p);
                        let number = std::str::from_utf8(&file.bytes[start..end])
                            .ok()
                            .and_then(|s| s.parse::<f64>().ok());
                        if let Some(number) = number {
                            file.pos = end;
                            Value::Number(number)
                        } else {
                            Value::Nil
                        }
                    }
                    _ => return Err(mlua::Error::runtime("unsupported file read format")),
                },
                _ => return Err(mlua::Error::runtime("unsupported file read format")),
            };
            let eof = value == Value::Nil;
            values.push_back(value);
            if eof {
                break;
            }
        }
        Ok(values)
    }

    fn lines(&self, lua: &Lua, formats: MultiValue) -> mlua::Result<Function> {
        let file = self.clone();
        lua.create_function(move |lua, ()| file.read(lua, formats.clone()))
    }
}

impl UserData for SandboxFile {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("read", |lua, file, formats: MultiValue| {
            file.read(lua, formats)
        });
        methods.add_method("lines", |lua, file, formats: MultiValue| {
            file.lines(lua, formats)
        });
        methods.add_method("write", |_, file, _: MultiValue| Ok(file.clone()));
        methods.add_method("flush", |_, _, ()| Ok(true));
        methods.add_method("close", |_, file, ()| {
            file.0.borrow_mut().closed = true;
            Ok(true)
        });
        methods.add_method(
            "seek",
            |_, file, (whence, offset): (Option<String>, Option<i64>)| {
                let mut file = file.0.borrow_mut();
                if file.closed {
                    return Err(mlua::Error::runtime("attempt to seek a closed file"));
                }
                let base = match whence.as_deref().unwrap_or("cur") {
                    "set" => 0,
                    "cur" => file.pos as i64,
                    "end" => file.bytes.len() as i64,
                    _ => return Err(mlua::Error::runtime("invalid seek origin")),
                };
                let pos = base.saturating_add(offset.unwrap_or(0));
                if pos < 0 {
                    return Err(mlua::Error::runtime("invalid seek offset"));
                }
                file.pos = (pos as u64).min(usize::MAX as u64) as usize;
                Ok(pos)
            },
        );
        methods.add_meta_method("__close", |_, file, _: MultiValue| {
            file.0.borrow_mut().closed = true;
            Ok(())
        });
    }
}

fn open(
    policy: &ReadPolicy,
    home: Option<&std::path::Path>,
    path: &str,
    mode: Option<&str>,
) -> Result<SandboxFile, String> {
    let mode = mode.unwrap_or("r");
    if mode.contains(['w', 'a', '+']) {
        return Ok(SandboxFile::new(Vec::new()));
    }
    if mode != "r" && mode != "rb" {
        return Err("invalid file mode".into());
    }
    policy
        .read(&expand_home(path, home))
        .map(|(_, bytes)| SandboxFile::new(bytes))
        .map_err(|e| e.to_string())
}

pub(super) fn install(lua: &Lua, policy: ReadPolicy, home: Option<PathBuf>) -> mlua::Result<Table> {
    let io = lua.create_table()?;
    // File bytes live in Rust userdata, outside Lua allocator accounting.
    let bytes_read = Rc::new(Cell::new(0usize));
    let open_bytes = bytes_read.clone();
    let open_policy = policy.clone();
    let open_home = home.clone();
    io.set(
        "open",
        lua.create_function(move |lua, (path, mode): (String, Option<String>)| {
            match open(&open_policy, open_home.as_deref(), &path, mode.as_deref()) {
                Ok(file) => {
                    account_read(&open_bytes, &file);
                    Ok((Value::UserData(lua.create_userdata(file)?), Value::Nil))
                }
                Err(e) => Ok((Value::Nil, Value::String(lua.create_string(e)?))),
            }
        })?,
    )?;
    io.set(
        "lines",
        lua.create_function(move |lua, (path, formats): (Option<String>, MultiValue)| {
            let file = match path {
                Some(path) => open(&policy, home.as_deref(), &path, Some("r"))
                    .map_err(mlua::Error::runtime)?,
                None => SandboxFile::new(Vec::new()),
            };
            account_read(&bytes_read, &file);
            file.lines(lua, formats)
        })?,
    )?;
    io.set(
        "popen",
        lua.create_function(|_, _: MultiValue| Ok((Option::<bool>::None, DISABLED)))?,
    )?;
    io.set(
        "read",
        lua.create_function(|_, _: MultiValue| Ok(Value::Nil))?,
    )?;
    for name in ["write", "flush", "close", "input", "output", "tmpfile"] {
        io.set(
            name,
            lua.create_function(|_, _: MultiValue| Ok(Value::Nil))?,
        )?;
    }
    Ok(io)
}

fn account_read(total: &Cell<usize>, file: &SandboxFile) {
    let bytes = total.get().saturating_add(file.0.borrow().bytes.len());
    if bytes > 8 * 1024 * 1024 {
        super::abort("file read memory limit exceeded");
    }
    total.set(bytes);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_formats_seek_lines_and_discard() {
        let lua = Lua::new();
        let file = SandboxFile::new(b"12\nhello\nbye".to_vec());
        lua.globals()
            .set("f", lua.create_userdata(file).unwrap())
            .unwrap();
        lua.load("assert(f:read('n') == 12); assert(f:read('*l') == ''); assert(f:read(2) == 'he'); assert(f:seek('set', 3)==3); assert(f:read('*l')=='hello'); assert(f:read('a')=='bye'); assert(f:read('l')==nil); f:close()").exec().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("never-created");
        let fake = open(
            &ReadPolicy::new([dir.path().to_owned()]),
            None,
            path.to_str().unwrap(),
            Some("w+"),
        )
        .unwrap();
        lua.globals()
            .set("f", lua.create_userdata(fake).unwrap())
            .unwrap();
        lua.load("f:write('discard me'); f:flush(); f:close()")
            .exec()
            .unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn lines_and_reading_past_eof_are_harmless() {
        let lua = Lua::new();
        let file = SandboxFile::new(b"\r\n1\n2\n".to_vec());
        lua.globals()
            .set("f", lua.create_userdata(file).unwrap())
            .unwrap();
        lua.load("f:seek('set',1); assert(f:read('l')==''); local count=0; for line in f:lines() do count=count+1 end; assert(count==2); f:seek('set',100); assert(f:read('n')==nil)").exec().unwrap();
    }
}

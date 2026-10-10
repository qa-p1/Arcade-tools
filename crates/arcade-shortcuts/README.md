# arcade-shortcuts

Worker-side shortcut readers and the schema-1 overlay index for Arcade Tools.
Rust 1.88, vendored Lua 5.5 (`mlua` 0.12). No UI integration or background polling.

See [VALIDATION.md](VALIDATION.md) for check results, merge statistics and
measured rebuild times.

```rust,no_run
use arcade_shortcuts::{build_index, default_index_path, hyprland, BuildInputs};

let inputs = BuildInputs {
    catalog_dir: Some("catalog-cache/shortcuts".into()),
    arcade_docs: vec!["docs-cache/arcade.find/shortcuts.json".into()],
    registry_dir: Some("registry/arcade/apps".into()),
    hyprland: Some(hyprland::read_machine(&Default::default())),
    user_labels: Some("tools/shortcut-labels.json".into()),
    ..Default::default()
};
let index = build_index(&inputs);
index.write_index(&default_index_path()?)?;
# Ok::<(), std::io::Error>(())
```

Run this entire operation on a worker. `build_index` performs disk reads and
never queries Hyprland; `read_machine` is the convenience config + runtime
reader. `lua::evaluate`, `conf::parse`, `runtime::parse_json` and
`hyprland::merge` accept separate inputs for tests or integrations with their
own runtime transport. `HyprlandReport.files` lists read config files for
change-triggered watching. The caller should also watch parent directories of
optional missing modules to detect their creation.

`SandboxOptions` supplies canonical read roots, environment overrides and VM
limits without changing process environment variables. Defaults are 64 MiB
of Lua memory, 50 million instructions, and a 100 ms evaluation deadline.
Sources, file-handle data, descriptor traversal and recorded binds have
separate Rust-side bounds. Module errors and top-level errors preserve prior
binds with warnings. Limit aborts cannot be swallowed by Lua `pcall`/`xpcall`.
Process execution, network, real file writes, binary chunks, native loaders,
debug and coroutine libraries are inaccessible. IO writes use discard handles.
`os.clock` measures elapsed evaluation time; calendar functions use host time.

Labels prefer `submap|chord` user overrides, descriptions, direct comments on
function binds, named key variables, then dispatcher descriptions. Source
comments provide groups; bindings without groups appear under Other. Runtime
matches consume each registration once, in config order. Unmatched runtime
`__lua` callbacks intentionally retain an honest generic label: Hyprland's
JSON does not expose their function source or dispatcher arguments.

Catalog and Arcade docs select this OS's keys (including explicit null), then
default; arrays remain alternatives and space-separated chords remain
sequences. Registry accelerators supply effective globals and docs supply
titles/rebindability when the manifest omits those fields. Docs may include an
OS `match`; absent Arcade matches fall back to the canonical/hyphenated app ID
on Linux, executable basename on Windows, and bundle/app ID on macOS.
Conflicts include equal chords and strict sequence prefixes across Arcade
globals and WM binds. OS-reserved lists are not included yet.

`Index::write_index` writes a same-directory temporary file, flushes and syncs
it, then atomically replaces the index. `default_index_path` honors
`ARCADE_HOME/cache`, Linux `XDG_CACHE_HOME`, or the platform cache directory,
followed by `arcade/tools/shortcuts-index.json`. Invalid source files are
isolated and their diagnostics join `wm.warnings`.

Read the real desktop (JSON on stdout, counts/timings on stderr):

```sh
cargo run -p arcade-shortcuts --release --example dump --locked -- \
  --catalog /path/to/Arcade-catalog/shortcuts
```

Additional example options: `--registry`, repeatable `--docs`, `--labels`, and
`--output` for an explicitly chosen generated index. Without `--output` it
writes no index. Only `hyprctl binds -j` is executed, with a one-second timeout
and only when `HYPRLAND_INSTANCE_SIGNATURE` is set.

Fixture timing, including config evaluation, 31 sheets, docs, manifests,
serialization, file sync and atomic rename:

```sh
cargo test -p arcade-shortcuts --release --locked --test fixtures \
  measure_owner_and_31_catalog_sheets -- --ignored --nocapture
```

Linux Hyprland was exercised on the owner's desktop. Windows/macOS key
selection and display are fixture-tested; those desktops were not exercised.

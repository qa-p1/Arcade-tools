# arcade-shortcuts

Worker-side shortcut readers and the schema-1 overlay index for Arcade Tools.
Rust 1.88, vendored Lua 5.5 (`mlua` 0.12). No UI integration or background polling.

The default `builder` feature enables config readers and index publication.
Consumers of an existing index can disable default features to share
`index::Index`, key normalization, and label helpers without Lua or the
builder dependencies. Readers ignore unknown fields, accept `wm: null` as an
empty WM section, and accept omitted/null app-group contexts.
`labels::default_path` uses the same config root as Tools, including
`ARCADE_HOME/config`; `Index::apply_user_labels` updates WM labels and conflict
participants after publication.

See [VALIDATION.md](VALIDATION.md) for check results, merge statistics and
measured rebuild times.

```rust,no_run
use arcade_shortcuts::{build_index, default_index_path, wm, BuildInputs};

let inputs = BuildInputs {
    catalog_dir: Some("catalog-cache/shortcuts".into()),
    arcade_docs: vec!["docs-cache/arcade.find/shortcuts.json".into()],
    registry_dir: Some("registry/arcade/apps".into()),
    wm: Some(wm::read_machine(&Default::default())),
    user_labels: Some("tools/shortcut-labels.json".into()),
    ..Default::default()
};
let index = build_index(&inputs);
index.write_index(&default_index_path()?)?;
# Ok::<(), std::io::Error>(())
```

Run this entire operation on a worker. `build_index` performs disk reads and
never queries the desktop; `wm::read_machine` selects the current desktop and
reads it once. The legacy `BuildInputs.hyprland` still works; `wm` takes
precedence. `lua::evaluate`, `conf::parse`, `runtime::parse_json` and
`hyprland::merge` accept separate inputs for tests or integrations with their
own runtime transport. `HyprlandReport.files` lists read config files for
change-triggered watching. The caller should also watch parent directories of
optional missing modules to detect their creation.

`SandboxOptions` supplies canonical read roots, environment overrides and VM
limits without changing process environment variables. Defaults are 64 MiB
of Lua memory and a primary limit of 50 million instructions. A configurable
two-second wall-clock backstop allows scheduling delays on busy machines.
Time aborts explicitly mark config labels incomplete in `wm.warnings`; merging
with runtime binds preserves every active registration, even when evaluation
only captures part of the config. Resident callers can schedule a delayed retry.
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
writes no index. Hyprland executes only `hyprctl binds -j`, with a one-second
timeout and only when `HYPRLAND_INSTANCE_SIGNATURE` is set.

Other Linux desktops are fixture-tested and **not run on a real desktop**:

- Sway reads `swaymsg -r -t get_binding_modes`, `get_config`, and `get_version`.
  The config parser handles variables, quoted words, continuations, bind
  blocks/flags, keycodes, includes/globs and modes. Includes use bounded,
  canonical read roots and reject symlink escapes. Comments and dispatcher
  descriptions supply labels. IPC mode names do not prove each binding is
  registered, so entries remain `configuredOnly`.
- KDE reads `<config>/kglobalshortcutsrc`. Current keys supply alternatives
  and sequences; empty/`none` keys stay disabled even when a default is present.
  KConfig escapes, component/context names and action labels are preserved.
- GNOME reads effective media-keys and WM keybindings with
  `gsettings list-recursively`, plus up to 16 enabled custom shortcut schemas.
  GTK modifiers and GVariant strings/arrays are parsed without evaluation.
  When GSettings cannot be read, `dconf dump /org/gnome/` supplies saved
  overrides only; schema defaults are explicitly unavailable.

Sway/GNOME subprocess output is capped at 2 MiB and each query at 500 ms.
KDE/GNOME runtime registration is not verified, so their entries are also
`configuredOnly`. The selected desktop name, groups, sources, warnings, labels
and Arcade-global/sequence-prefix conflicts use the same index pipeline as
Hyprland. No reader executes a configured action or writes WM settings.
`WmReport.files` lists files already read for caller-owned event watching;
no polling or resident lifecycle is added.

Formats follow upstream [Sway IPC](https://github.com/swaywm/sway/blob/master/sway/sway-ipc.7.scd),
[KGlobalAccel settings](https://invent.kde.org/plasma/kglobalacceld/-/blob/master/src/component.cpp),
and [GNOME media-keys schemas](https://gitlab.gnome.org/GNOME/gnome-settings-daemon/-/blob/master/data/org.gnome.settings-daemon.plugins.media-keys.gschema.xml.in).

Fixture timing, including config evaluation, 31 sheets, docs, manifests,
serialization, file sync and atomic rename:

```sh
cargo test -p arcade-shortcuts --release --locked --test fixtures \
  measure_owner_and_31_catalog_sheets -- --ignored --nocapture
```

Linux Hyprland was exercised on the owner's desktop. Windows/macOS key
selection and display are fixture-tested; those desktops were not exercised.

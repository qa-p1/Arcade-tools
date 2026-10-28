---
title: Preview, browse and extend files
order: 3
since: 0.2.0
---

# Preview, browse and extend files

## Open a file or folder

Open a file with Arcade Look from your file manager, use its preview shortcut, run `arcade-look path/to/file`, or drop a file or folder on the Look window. In GNOME Files, Space uses the GNOME previewer integration. On Windows, File Explorer can use Space when the integration is enabled. On macOS, Finder uses Control+Option+Space for Look so Apple's Space preview remains intact.

A folder opens as a browsable grid. Click a subfolder to enter it, then use the title-bar Back and Forward buttons, **Backspace**, **Alt+Left**, **Alt+Right**, or your mouse's Back and Forward buttons to move through navigation history. **Left** and **Right** move between files in the current folder. Hidden files are omitted by default; the `showHidden` setting includes them.

## Read the file that fits the task

Look selects a viewer based on file type and available decoders. It supports image and camera RAW previews; video and audio; PDF; DOCX, ODT, RTF and EPUB; CSV and common Excel/OpenDocument spreadsheets; PPTX and ODP; Markdown, source code, JSON, JSON Lines, GeoJSON and Jupyter notebooks; fonts; 3D models; HTML; folders; and ZIP/TAR-family archives. Select an archive entry to preview it in place.

### Review pictures, documents and data

Images can zoom toward the pointer, pan, rotate, fit the window, show actual size, and display transparency on a checkerboard. Look shows available image dimensions, EXIF and GPS details. PDF pages render as needed and support selectable text, zoom and page navigation. Documents and EPUBs show formatted text, headings, tables, links and embedded images. Spreadsheets use a virtualized grid with sheet tabs; CSV and TSV delimiter and encoding are detected. Slides appear as cards with titles, bullets, images, tables and speaker notes.

### Read code and inspect archives

Code and text previews have line numbers, line wrapping and adjustable text size, with UTF-8/16 and legacy-encoding detection. Markdown renders GitHub-flavored content and can switch to source. JSON is shown as a collapsible tree; notebooks include Markdown, code and text, image or HTML outputs. HTML is rendered in a sandbox without scripts, with a source view. Archive entries can be opened in place without extracting the archive yourself.

### Inspect media, fonts and models

Video and audio have playback, seeking, speed, loop and mute controls; audio can show cover art, tags and a waveform. Playback depends on codecs supplied by the operating system. Font previews include an editable sample, size range, variable-axis sliders and a glyph grid. 3D models support orbiting, auto-rotation, wireframe, grid, reset, PBR lighting and available animations.

If a format has no supported viewer or its decoder fails, Look falls back to a file information card and a hex view. A recognized file can still lack a rich preview when its format decoder or an operating-system codec is unavailable.

## Inspect, zoom and navigate a preview

Use the **Info** button or **I** to show file details. Images support zoom, pan, rotation, fit-to-window and actual-size views. PDF pages support zoom and page navigation. Text and code can wrap long lines and change reading size. A 3D model can rotate, show a wireframe or reset its view. Video and audio have playback, seeking, speed, loop, mute and volume controls.

Press **Enter** to open the current file in its default application. Use **Ctrl+O** (or **Command+O** on macOS) for the same action. **Ctrl+R** reveals the file in its containing folder, and **Ctrl+C** copies its path when no text is selected. **F** toggles fullscreen. **Space** or **Escape** closes the preview; **Escape** exits fullscreen first. See the [shortcut catalog](shortcuts.json) for viewer-specific keys.

## Add a preview plugin

Plugins add preview support for formats that Look does not handle directly. A command plugin runs a program and can return HTML, text, Markdown, an image, or a file that Look opens with a built-in viewer. A script plugin is an ES module that renders into the preview pane.

Place plugin definitions in Look's plugins directory and restart Look to load them. Command plugins are launched as an argument list rather than through a shell; use a command such as `sh -c` explicitly when a pipeline is needed. Script plugins run with the same privileges as built-in viewers, so install only scripts you trust. See the [plugin reference](https://github.com/qa-p1/Arcade-look/blob/main/docs/PLUGINS.md) for supported fields and paths.

## Use connected Arcade actions

When a compatible peer is installed and enabled, press **A** or open the Actions button. Look can pass the current file (or the current PDF page for supported actions) to Box, Lens, Clipboard, Wheel or Shelf. Results returned by an action can open in Look. Configure peers and inspect their status in **Settings → Connected apps**. See [working with other Arcade apps](with-other-apps.md).

## Change settings

Choose the theme, idle timeout, autoplay, file-manager integration, plugin loading and connected apps from Settings. Settings are stored in `config.json` in the platform's per-user configuration folder. When a preview or Settings is hidden, Look keeps its window warm for 30 seconds, then releases the webview. Background integrations, the tray and start-at-login keep the process available afterward. Otherwise the `idleMinutes` config option controls when the process exits (0 disables that exit, but still releases the webview); other Arcade apps start Look again when they need it.

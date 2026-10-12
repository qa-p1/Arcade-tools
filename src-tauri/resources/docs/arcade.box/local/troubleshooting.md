---
title: Troubleshooting
order: 6
since: 0.1.2
---

# Troubleshooting

## The shortcut does not open Box

Open Box Settings and check the global-shortcut status. Another app may already own the key combination; choose and save a different one. On Wayland, the desktop’s GlobalShortcuts portal must accept the binding. A permission prompt may appear when you save.

## A screen task does not open a picker

Screen capture depends on your desktop’s capture portal or native picker. Box’s full Wayland selection and recording flows still need a real desktop verification. Try the action from a supported session and check whether the desktop displays a permission prompt.

## A tool says an engine is missing or still checking

Open **Settings → Engines** to see which programs Box detected. Install the missing program with your system package manager, then refresh the engine list. The first engine scan on a new profile can take about six seconds; tools that need an engine may show “still checking” during that time.

## OCR does not find the text

OCR needs Tesseract or an available Lens provider. Small text can be misread. Check the provider status, then try a larger or clearer image. Searchable PDF OCR additionally needs Poppler and qpdf.

## Webpage-to-PDF output has no selectable text

When Box uses a Firefox-family browser, it saves the page as images, so the resulting PDF does not contain selectable text. A compatible Chromium-family browser produces a text PDF.

## A long-running action seems to disappear

Jobs can continue after you close the Island. Reopen Box and check **Background jobs** for progress or results.

## The command is not found on Linux

Box installs **arcade-box** in **~/.local/bin**. If that directory is not on PATH, use the exact shell line shown by the installer, then open a new terminal.

## I am on Windows or macOS

The builds and installers pass CI, but neither platform has been run interactively. If a desktop-specific issue occurs, include your operating-system version and the relevant app log when reporting it. Avoid treating a successful build as proof that a desktop flow is verified.

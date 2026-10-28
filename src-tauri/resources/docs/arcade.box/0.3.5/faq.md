---
title: FAQ
order: 5
since: 0.2.0
---

# Frequently asked questions

## Does Box install FFmpeg or other tools for me?

No. Box detects external programs such as FFmpeg, Poppler, qpdf, libvips, ImageMagick, Ghostscript, yt-dlp, and eSpeak NG. It does not silently install them. Check **Settings → Engines** to see what is available. Tesseract can be downloaded explicitly from that page and shared with other Arcade apps.

## Does every tool work on every computer?

No. A task may need a particular engine, browser, permission, input format, or operating-system feature. Box explains why an action is unavailable. The tool matrix records which cases have been verified.

## Does Box upload my files?

Most actions process files locally. Some tasks make network requests, such as downloads, webpage checks, or speech transcription through Groq. Box identifies network or command effects and asks for approval before running them. Transcription needs your Groq API key.

## Why did a PDF lose its images or layout?

The built-in document-to-PDF converter keeps text and basic formatting but does not preserve images or exact page layout. Older .doc and .ppt files must first be saved in a newer format. Use a compatible installed converter when exact layout matters.

## Where are my recent tools and settings stored?

The desktop app and command-line CLI use separate databases. A recent action or favorite in one does not automatically appear in the other.

## Has Box been tested on Windows and macOS desktops?

No. Windows and macOS builds and installers pass CI, but the app has not been run interactively on either platform. On Linux, Box’s window pin and plain-text paste were exercised on Hyprland; the full Wayland screen-selection and recording flows still need a desktop run.

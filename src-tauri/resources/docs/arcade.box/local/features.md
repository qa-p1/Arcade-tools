---
title: Features
order: 3
since: 0.1.2
---

# Features

Box has 109 catalog tools. Search by the task you want to do; availability depends on your input, platform, permissions, and optional programs. Tools that need local engines do not install them silently.

## Work with PDFs

- Merge PDFs; split a PDF or extract selected pages; reorder, rotate, or remove pages.
- Compress a PDF, add a signature, fill a PDF form, add a watermark or page numbers, and protect or unlock a PDF.
- Turn images into a PDF, render PDF pages as images, convert a document to PDF, and extract text, images, metadata, or attachments.
- Make a scanned PDF searchable with OCR. This needs Poppler, Tesseract, and qpdf.

Document-to-PDF conversion keeps text, headings, lists, bold text, and tables, but not images or exact page layout. Save older .doc or .ppt files as .docx or .pptx first.

## Edit and convert images

- Convert formats, resize or resample, crop, rotate, flip, compress, and combine images.
- Remove a background, make a passport or ID photo, add a watermark, generate a favicon or app icon, or upscale and enhance an image.
- Blur, pixelate, or redact a region; remove metadata; read text with OCR; extract a color palette; compare two images.
- Box can use libvips or ImageMagick for image work. Some actions need one of these programs.

## Edit and inspect video

- Convert or transcode video, compress toward a target size, trim, crop, resize, rotate, and join clips.
- Extract audio or frames; create a GIF or animated WebP; edit subtitles; make subtitles automatically; inspect media metadata and streams.
- Use the timeline to seek or choose a trim range. Use left/right or up/down arrows to move a handle, Shift with an arrow for a finer step, and Home or End to move to the start or end.
- FFmpeg and ffprobe power many video actions. Subtitle transcription and automatic subtitles use Groq and require **GROQ_API_KEY**.

## Work with audio and speech

Convert, trim, or join audio; normalize loudness; reduce background noise; remove vocals; adjust speed or pitch; trim or split silence; edit audio metadata and cover art; transcribe audio or video; or turn text into speech.

Audio conversion uses FFmpeg. Transcription uses Groq and needs an API key. Text-to-speech uses the operating system’s voice: SAPI on Windows, say on macOS, or eSpeak NG/eSpeak on Linux.

## Download and inspect web content

- Download media, a video transcript or subtitles, a video thumbnail, all images from a page, or a direct file URL.
- Check a website, save a webpage as PDF or a screenshot, extract webpage text, look up WHOIS or domain information, and check your public IP address.
- Media downloads may use yt-dlp. Webpage capture reuses a compatible installed browser. These actions connect to the network and ask for approval where required.

## Transform text and data

Compare or merge text; change case; clean text; convert JSON, YAML, TOML, and XML; edit CSV or TSV; export between Markdown and HTML; check text length and readability; or generate Lorem Ipsum.

Box also includes a calculator; percentage, discount, tax, and tip calculations; loan and EMI estimates; unit, currency, time-zone, and date/time conversion; color conversion and contrast checks; and Excel-to-CSV or CSV-to-Excel conversion.

## Use everyday utilities

Pick a random item from a list, generate a password, use the emoji picker, or run a timer or stopwatch. These are available from the same searchable Island as Box’s file and conversion tools.

## Create and decode codes

Generate or read QR codes and barcodes, including batch barcode labels.

## Use developer utilities

Test regular expressions; convert between number bases and binary encodings; encode, decode, or inspect URL queries; calculate hashes and checksums; generate UUIDs, ULIDs, and other identifiers; inspect JWTs; and build or explain cron expressions.

## Manage files and the desktop

- Batch rename files, find duplicates, compare files or folders, inspect disk and folder sizes, and encrypt or decrypt files.
- Capture a screenshot, record the screen, recognize text from the screen, scan a screen for QR codes or barcodes, sample a screen color, measure screen distances, or pin a frozen screenshot.
- Open clipboard history, paste plain text, pin a window, find or terminate a process, and inspect system information.
- On Linux, Box’s window-pin and paste-plain actions have been exercised on Hyprland. Full Wayland screen selection and recording still need a desktop run.

## Save work as a pipeline

Save compatible actions as a pipeline and run it again from search. Pipelines can pass files and values between steps, show progress, and keep long-running jobs going while the Island is closed. A pipeline that invokes a peer app may open that app’s picker. Review and approve network, command, or other outbound effects when asked.

## Use plugins

The Plugins page can install community plugins that run in a restricted WebAssembly worker. Review each plugin’s declared permissions before installing it. Plugins are optional; Box’s built-in tools remain available without them.

For tool input formats and detailed limits, see the [tool catalog](../tool-catalog.md), [tool matrix](../tool-matrix.md), and [provider notes](../providers.md).

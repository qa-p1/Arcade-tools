---
title: Privacy and security
order: 6
---

# Privacy and security

Arcade apps and Arcade Tools do not send telemetry or usage analytics. App
settings, indexes, history, and saved actions stay in your user folders unless
you use a feature that sends content somewhere.

## Arcade Link stays on this computer

Link uses local sockets on Linux and macOS and named pipes on Windows. Apps
find one another through per-user registry files and authenticate with a
per-user token. Link does not use an Arcade cloud service or relay. It passes
file paths or short-lived handoff files between local apps.

Link protects against other operating-system users, but it does not prove that
a process running as your user is an official Arcade app. Keep your account and
device secure. Each receiving app still applies its own confirmation and
privacy rules.

## Network activity you choose

- **Arcade Tools** checks the catalog at raw.githubusercontent.com and checks
  app releases through GitHub. Downloads use HTTPS and are verified against
  the release's declared size and SHA-256 checksum before install. A checksum
  checks integrity; it is not a publisher signature. Tools does not remove
  macOS quarantine or bypass Windows SmartScreen.
- **Arcade Clipboard** syncs clips to devices you pair and approve. Devices
  connect directly over the local network when they can, or through a relay
  address you configure. Text, links, formatted text, images, and files travel
  inside end-to-end encrypted connections. Files are sent as content, not as
  paths to your disk. A relay forwards encrypted traffic without reading or
  storing clips, but can see device addresses, connection timing, and traffic
  volume. Private mode stops capture, and receiving a clip never changes your
  system clipboard by itself.
- **Arcade Box** makes network requests only when you run a network-capable
  tool, such as a web download, webpage conversion, site check, IP/domain
  lookup, or cloud speech transcription. Speech tools send the selected audio
  to Groq when you configure and use a Groq key. Box asks before first use of
  outbound, network, or command effects, including in a pipeline.
- **Arcade Lens** can open configured web search, map, product, and lookup
  providers with the finding you selected. Reverse-image search is off by
  default. If you enable it, Lens copies the selected image to your clipboard
  and opens the service; you paste it there yourself. Lens does not upload the
  image. Its secret check hides outbound actions for likely passwords or keys.
- **Arcade Look** previews local files and has no file-sync service. Its Link
  URL input accepts file URLs only. Opening a web or mail link hands it to your
  default browser or mail app. Some 3D files can reference external resources;
  previewing one may cause the renderer to request those URLs.

Other actions such as local file conversion, indexing, and previewing do not
send their content to Arcade. Check the action's own description before using
a network-enabled tool or provider.

## Diagnostics

A diagnostics archive is assembled on your computer. It is not uploaded by
Arcade. Review it before sharing it yourself, because it can include system
details and log files.

The app catalog restricts managed app repositories to the qa-p1 owner. Tools
uses release metadata and checksums but currently does not verify publisher
signatures. See [Installing and updating](installing.md) for the platform
warnings, and [uninstalling and your data](uninstalling.md) for local storage
locations.

---
title: How the apps connect
order: 3
---

# How the apps connect

Arcade Link lets Arcade apps on the same computer offer actions to one another.
It is local communication between apps running as your user; it does not send
Link requests through an Arcade server or the internet.

Open an app's **Connected apps** page to manage Link. Its master switch,
**Connect with other Arcade apps**, turns that app's Link actions on or off.
Each row's **Use with Arcade <name>** switch controls whether that peer's
actions appear in this app. Turning Link off hides the app's actions and stops
its Link listener. The app's own features continue to work.

Apps find each other through small local registry files and connect directly
using a per-user local socket on Linux and macOS or a named pipe on Windows.
When an action needs a file or a piece of text, Link passes a local path or a
short-lived handoff file. The receiving app uses its usual safety checks.
Missing, disabled, or unavailable peers simply do not add an action.

## Things you can do

These flows are implemented in the apps and described in the Link protocol and
app documentation:

- **Send a screen clip to your devices.** Select an area in Lens and choose
  **Send to my devices**. Clipboard syncs the image to your paired devices.
  Lens hides the send action when its secret check finds a likely password or
  key.
- **Build a reusable screenshot action.** A Box pipeline can capture with
  Lens, resize and convert the image in Box, then add it to Clipboard. Save
  the pipeline to a Wheel slot to run it with one gesture.
- **Work with a clip from history.** From Clipboard, preview an image or file
  with Look, extract image text or pin it with Lens, or convert it with Box.
  Results can become new clips; receiving a clip never overwrites the system
  clipboard.
- **Compress a file you are previewing.** In Look, choose a Box action such as
  **Compress for sharing**. The result opens back in Look, where an eligible
  file can be sent through Clipboard.
- **Use Lens with Box.** Box can ask Lens to capture a screen region or provide
  OCR. Without Lens, Box keeps its own available capture and OCR paths.
- **Save an action from Lens to Wheel.** Choose **Add to Wheel** for a finding.
  Wheel opens its settings with the action ready for you to choose a slot and
  save; Wheel does not execute command text through a shell.
- **Use results from Find.** Find can offer compatible results to Look, Box,
  Clipboard, Lens, Shelf, and Wheel. For example, preview a file, run a
  compatible Box action, or add a selection to Shelf.

An action appears only when its peer is installed, enabled, supports the
current input and operating system, and reports that action as available.
Outbound actions show a preview and keep the receiving app's confirmation or
privacy rules. Box asks before first use of outbound, network, or command
effects; a pipeline confirmation covers the effects of its combined steps.

These platform flows are not equally verified. The catalog labels every
Windows and macOS app **ci**, not **verified**; those platforms have CI builds
and tests but no real-desktop run. See the
[platform note in Installing and updating](installing.md).

For the shared tray, see [One tray icon](tray.md). For what leaves your
computer, see [Privacy and security](privacy.md).

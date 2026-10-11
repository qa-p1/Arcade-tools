---
title: Glossary
order: 9
---

# Glossary

- **Adopt:** Take over an app that was installed outside Tools so Tools can
  manage that existing installation. The current Tools build does not offer
  adoption: it can launch unmanaged apps, but cannot update or remove them as
  managed installs.
- **Bundle:** A named selection of Arcade apps in the catalog, such as
  Essentials or Screen and capture. It is a group of apps, not a single
  installer file.
- **Channel:** The release stream used for an app. **Stable** is the normal
  release; **nightly** is a more frequent development build. Tools chooses a
  channel per app.
- **Engine:** A helper program an app can use for a task, such as Tesseract
  for OCR. Engines are separate from the Arcade app and may be shared.
- **Handoff:** A short-lived, private file used when one local app gives
  another app content that does not fit in a path or small text message.
- **Arcade Link:** The local protocol apps use to find one another and request
  actions. It uses local IPC and does not require a central service.
- **Receipt:** Tools' local record of an installation, including its app,
  version, channel, and package type. Tools uses it to update, repair, or
  uninstall the copy it manages.
- **Registry:** The per-user folder of small Arcade Link files that describe
  installed apps and their available actions. It is not a cloud account or
  online directory.
- **Tray hosting:** The option in Tools that gathers running Arcade apps'
  tray icons under one Arcade icon. Turn it off to restore each app's own icon.
- **Stable / nightly:** The two release channels. Stable favors completed
  releases; nightly carries newer builds and may change more often.

For setup and updates, see [Installing and updating](installing.md). For Link,
see [How the apps connect](how-apps-connect.md).

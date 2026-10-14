---
title: Set up decks, actions and gestures
order: 3
since: 0.2.0
---

# Set up decks, actions and gestures

## Arrange actions into decks

A deck has up to eight radial slots. Create as many decks as you need, name them, duplicate them, move them earlier or later, and delete them. The first **Applications** deck is filled with up to six installed graphical apps; empty slots are ready to edit. Choose a slot in **Settings → Wheel** to add, replace or clear an action.

Scroll while holding the trigger to switch decks. Decks wrap around by default; **Settings → Behaviour** can turn wrapping off or reverse scroll direction. The wheel shows the current deck and its position in the deck list.

## Choose what a slot does

The action picker can launch or focus an application, run a command with arguments, open a URL, file or folder, perform a supported system action, send a media key, run a compositor action, or invoke a compatible Arcade app action. Commands run as a program and argument list, not through a shell; use `sh -c` explicitly if a shell command is needed.

A slot whose application, provider or action is missing stays visible in Settings with its unavailable reason. Repair the slot or reconnect the provider to use it again.

## Invoke the wheel

Hold the configured global trigger. The overlay opens on the screen containing the pointer and, on Hyprland, moves the pointer to the wheel center. Move toward a slice to select its action; the selected name appears in the hub. Release the trigger to run it. Releasing in the center cancels. Right-click or **Escape** cancels the overlay. Scroll to change decks while holding.

**Settings → Trigger** records a different trigger, checks for shortcut collisions and shows whether the platform registered it. You can set a hold threshold so Wheel waits before opening. The default threshold is zero, so the wheel opens as soon as the trigger is pressed.

## Assign center gestures

Open **Settings → Center gestures** to configure single-click, double-click, triple-click and long-press groups. Each group has a name, an enable switch and up to 16 actions that run in order. Groups are unassigned by default. Hold the trigger and click the center hub to perform the configured gesture; the wheel closes when the group runs.

Adjust the long-press duration and click interval in the same settings page. A single click can wait briefly while Wheel checks whether a double- or triple-click follows. Releasing the trigger completes a pending click immediately. Moving out of the center, switching decks or right-clicking cancels a pending gesture.

## Use Arcade actions and inputs

The **Arcade** section of the action picker lists compatible actions from installed peers. Depending on the target action, Wheel can pass no input, a snapshot of the current clipboard, a Lens selection, or a file selection resolved by Look. The picker only shows input modes supported by both the peer and the selection resolver. Look's selection resolver is currently absent on Linux, so that mode is hidden there.

Actions that send content outward show an effect marker and a preview. Their owning app applies its own privacy settings and confirmations. Use **Activity** to follow progress, read results or errors, and cancel a running request. See [working with other Arcade apps](with-other-apps.md).

Other apps can offer a saved action to Wheel. Wheel opens a draft in Settings; choose a deck and slot, then confirm. An incoming request never changes a deck on its own.

## Adjust the wheel and settings

**Settings → Appearance** changes wheel size, scale, center size, card size, labels, opacity, dimming, animation and reduced motion. The preview updates as you adjust values. **Settings → Behaviour** controls the center dead zone, selection sensitivity, scroll cooldown, deck wrapping, scroll direction and focusing an already-running app.

**Settings → General** can enable start at login and notifications. Settings are written to a versioned JSON config and apply while Wheel is running. Use the General page to import or export that config. The tray menu can reopen Wheel or Settings, restart the app, or quit it.

## Use Wheel on different desktops

Wheel is aimed at Linux Wayland, especially Hyprland, and also has Windows and macOS builds. On Wayland, the overlay uses LayerShellQt and the global trigger uses the XDG GlobalShortcuts portal. Hyprland can provide pointer position and movement; other Wayland desktops may only provide a best-effort position. Fullscreen games or applications with exclusive input can block the trigger or overlay.

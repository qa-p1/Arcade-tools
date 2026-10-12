use super::{
    assets,
    model::{Action, Item, TrayModel},
};
use std::sync::Arc;
type Dispatch = Arc<dyn Fn(Action) + Send + Sync>;

#[cfg(target_os = "linux")]
mod platform {
    use super::*;
    use ksni::blocking::{Handle, TrayMethods};

    pub struct Renderer(Handle<Tray>);
    struct Tray {
        model: TrayModel,
        dispatch: Dispatch,
    }

    impl ksni::Tray for Tray {
        fn id(&self) -> String {
            "arcade-tools".into()
        }
        fn title(&self) -> String {
            "Arcade Tools".into()
        }
        fn icon_pixmap(&self) -> Vec<ksni::Icon> {
            let rgba = assets::mark(self.model.badge);
            let mut data = Vec::with_capacity(rgba.len());
            for p in rgba.as_chunks::<4>().0 {
                data.extend_from_slice(&[p[3], p[0], p[1], p[2]]);
            }
            vec![ksni::Icon {
                width: 32,
                height: 32,
                data,
            }]
        }
        fn tool_tip(&self) -> ksni::ToolTip {
            ksni::ToolTip {
                title: "Arcade Tools".into(),
                ..Default::default()
            }
        }
        fn activate(&mut self, _: i32, _: i32) {
            (self.dispatch)(Action::OpenTools);
        }
        fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
            self.model.items.iter().map(menu).collect()
        }
    }

    fn menu(item: &Item) -> ksni::MenuItem<Tray> {
        use ksni::menu::{CheckmarkItem, StandardItem, SubMenu};
        match item {
            Item::Separator => ksni::MenuItem::Separator,
            Item::Submenu {
                title,
                glyph,
                items,
            } => SubMenu {
                label: title.replace('_', "__"),
                icon_data: assets::png(glyph).to_vec(),
                submenu: items.iter().map(menu).collect(),
                ..Default::default()
            }
            .into(),
            Item::Action {
                title,
                enabled,
                checked,
                action,
                ..
            } => {
                let action = action.clone();
                let activate = Box::new(move |t: &mut Tray| (t.dispatch)(action.clone()));
                match checked {
                    Some(checked) => CheckmarkItem {
                        label: title.replace('_', "__"),
                        enabled: *enabled,
                        checked: *checked,
                        activate,
                        ..Default::default()
                    }
                    .into(),
                    None => StandardItem {
                        label: title.replace('_', "__"),
                        enabled: *enabled,
                        activate,
                        ..Default::default()
                    }
                    .into(),
                }
            }
        }
    }

    impl Renderer {
        pub fn start(
            _: &tauri::AppHandle,
            model: TrayModel,
            dispatch: Dispatch,
        ) -> Result<Self, String> {
            // ksni listens for NameOwnerChanged and registers again when the
            // watcher appears, including login before the panel starts.
            Tray { model, dispatch }
                .assume_sni_available(true)
                .spawn()
                .map(Self)
                .map_err(|e| e.to_string())
        }
        pub fn update(&self, model: TrayModel) {
            let _ = self.0.update(|t| t.model = model);
        }
    }
    impl Drop for Renderer {
        fn drop(&mut self) {
            self.0.shutdown();
        }
    }
}

#[cfg(any(not(target_os = "linux"), feature = "native-tray-check"))]
#[cfg_attr(target_os = "linux", allow(dead_code))]
mod native {
    use super::*;
    use std::collections::BTreeMap;
    use tauri::{
        image::Image,
        menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu},
        tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
        AppHandle,
    };
    pub struct Renderer {
        app: AppHandle,
        actions: Arc<std::sync::Mutex<BTreeMap<String, Action>>>,
    }

    fn fill(
        app: &AppHandle,
        items: &[Item],
        append: &mut dyn FnMut(&dyn tauri::menu::IsMenuItem<tauri::Wry>) -> tauri::Result<()>,
        actions: &mut BTreeMap<String, Action>,
    ) -> tauri::Result<()> {
        for item in items {
            match item {
                Item::Separator => append(&PredefinedMenuItem::separator(app)?)?,
                Item::Submenu {
                    title,
                    glyph,
                    items,
                } => {
                    let submenu = Submenu::new(app, title, true)?;
                    submenu.set_icon(Some(Image::new_owned(
                        assets::rgba(glyph).to_vec(),
                        32,
                        32,
                    )))?;
                    fill(app, items, &mut |i| submenu.append(i), actions)?;
                    append(&submenu)?;
                }
                Item::Action {
                    id,
                    title,
                    enabled,
                    checked,
                    action,
                } => {
                    actions.insert(id.clone(), action.clone());
                    if let Some(checked) = checked {
                        append(&CheckMenuItem::with_id(
                            app,
                            id,
                            title,
                            *enabled,
                            *checked,
                            None::<&str>,
                        )?)?;
                    } else {
                        append(&MenuItem::with_id(app, id, title, *enabled, None::<&str>)?)?;
                    }
                }
            }
        }
        Ok(())
    }

    impl Renderer {
        pub fn start(
            app: &AppHandle,
            model: TrayModel,
            dispatch: Dispatch,
        ) -> Result<Self, String> {
            let actions = Arc::new(std::sync::Mutex::new(BTreeMap::new()));
            let menu = Menu::new(app).map_err(|e| e.to_string())?;
            fill(
                app,
                &model.items,
                &mut |i| menu.append(i),
                &mut actions.lock().unwrap(),
            )
            .map_err(|e| e.to_string())?;
            let event_actions = actions.clone();
            let menu_dispatch = dispatch.clone();
            TrayIconBuilder::with_id("arcade-tools")
                .icon(Image::new_owned(assets::mark(model.badge), 32, 32))
                .tooltip("Arcade Tools")
                .menu(&menu)
                .show_menu_on_left_click(cfg!(target_os = "macos"))
                .on_menu_event(move |_, event| {
                    if let Some(action) = event_actions
                        .lock()
                        .unwrap()
                        .get(event.id.as_ref())
                        .cloned()
                    {
                        menu_dispatch(action);
                    }
                })
                .on_tray_icon_event(move |_, event| {
                    if matches!(
                        event,
                        TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            ..
                        }
                    ) && !cfg!(target_os = "macos")
                    {
                        dispatch(Action::OpenTools);
                    }
                })
                .build(app)
                .map_err(|e| e.to_string())?;
            Ok(Self {
                app: app.clone(),
                actions,
            })
        }
        pub fn update(&self, model: TrayModel) {
            if let Some(tray) = self.app.tray_by_id("arcade-tools") {
                if let Ok(menu) = Menu::new(&self.app) {
                    let mut actions = BTreeMap::new();
                    if fill(
                        &self.app,
                        &model.items,
                        &mut |i| menu.append(i),
                        &mut actions,
                    )
                    .is_ok()
                    {
                        *self.actions.lock().unwrap() = actions;
                        let _ = tray.set_menu(Some(menu));
                        let _ = tray.set_icon(Some(Image::new_owned(
                            assets::mark(model.badge),
                            32,
                            32,
                        )));
                    }
                }
            }
        }
    }
    impl Drop for Renderer {
        fn drop(&mut self) {
            self.app.remove_tray_by_id("arcade-tools");
        }
    }
}

#[cfg(not(target_os = "linux"))]
pub use native::Renderer;
#[cfg(target_os = "linux")]
pub use platform::Renderer;

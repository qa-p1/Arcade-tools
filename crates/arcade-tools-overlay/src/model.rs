use crate::index::{Index, MISSING_MESSAGE};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    ThisApp,
    Arcade,
    WindowManager,
    All,
}

impl Scope {
    pub const ORDER: [Self; 4] = [Self::ThisApp, Self::Arcade, Self::WindowManager, Self::All];
    pub fn next(self, backwards: bool) -> Self {
        let index = Self::ORDER
            .iter()
            .position(|scope| *scope == self)
            .unwrap_or(0);
        Self::ORDER[(index + if backwards { 3 } else { 1 }) % 4]
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::ThisApp => "This app",
            Self::Arcade => "Arcade",
            Self::WindowManager => "Window manager",
            Self::All => "All",
        }
    }
}

pub struct Model {
    pub index: Option<Index>,
    pub app: Option<usize>,
    pub scope: Scope,
    pub query: String,
    pub picking: bool,
    pub picker_selected: usize,
    pub scroll: f32,
}

pub struct Row {
    pub title: String,
    pub detail: Option<String>,
    pub chips: Vec<String>,
    pub conflict: bool,
}

pub struct ViewGroup {
    pub title: String,
    pub rows: Vec<Row>,
}
pub struct Section {
    pub scope: Scope,
    pub title: String,
    pub groups: Vec<ViewGroup>,
}

impl Model {
    pub fn new(index: Option<Index>, app: Option<usize>) -> Self {
        let picking = index.is_some() && app.is_none();
        Self {
            index,
            app,
            scope: Scope::All,
            query: String::new(),
            picking,
            picker_selected: 0,
            scroll: 0.0,
        }
    }

    pub fn title(&self) -> &str {
        self.index
            .as_ref()
            .and_then(|index| self.app.and_then(|app| index.apps.get(app)))
            .map_or("All shortcuts", |app| app.name.as_str())
    }

    pub fn message(&self) -> Option<&'static str> {
        self.index.is_none().then_some(MISSING_MESSAGE)
    }

    pub fn type_text(&mut self, text: &str) {
        self.query.extend(text.chars().filter(|c| !c.is_control()));
        self.scope = Scope::All;
        self.scroll = 0.0;
        self.picker_selected = 0;
    }

    pub fn backspace(&mut self) {
        self.query.pop();
        self.scroll = 0.0;
        self.picker_selected = 0;
    }
    pub fn clear_query(&mut self) {
        self.query.clear();
        self.scroll = 0.0;
        self.picker_selected = 0;
    }
    pub fn cycle(&mut self, backwards: bool) {
        self.picking = false;
        self.scope = self.scope.next(backwards);
        self.scroll = 0.0;
    }
    pub fn open_picker(&mut self) {
        self.picking = true;
        self.clear_query();
        self.scope = Scope::All;
    }

    pub fn picker_candidates(&self) -> Vec<usize> {
        self.index
            .as_ref()
            .map(|index| {
                index
                    .apps
                    .iter()
                    .enumerate()
                    .filter(|(_, app)| {
                        matches_query(&format!("{} {}", app.name, app.id), &self.query)
                    })
                    .map(|(index, _)| index)
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn move_picker(&mut self, delta: i32) {
        let count = self.picker_candidates().len();
        if count > 0 {
            self.picker_selected =
                (self.picker_selected as i32 + delta).clamp(0, count as i32 - 1) as usize;
        }
    }

    pub fn pick(&mut self, candidate: Option<usize>) {
        if let Some(app) =
            candidate.or_else(|| self.picker_candidates().get(self.picker_selected).copied())
        {
            self.app = Some(app);
            self.picking = false;
            self.scope = Scope::All;
            self.clear_query();
        }
    }

    pub fn sections(&self) -> Vec<Section> {
        let Some(index) = &self.index else {
            return Vec::new();
        };
        let mut sections = Vec::new();
        if self.scope == Scope::All || self.scope == Scope::ThisApp {
            let all_apps = (self.app.is_none() && self.scope == Scope::All)
                || (!self.query.is_empty() && self.scope == Scope::All);
            let mut groups = Vec::new();
            for (position, app) in index.apps.iter().enumerate() {
                if !all_apps && self.app != Some(position) {
                    continue;
                }
                for group in &app.groups {
                    let rows: Vec<_> = group
                        .shortcuts
                        .iter()
                        .filter_map(|shortcut| {
                            let haystack = format!(
                                "{} {} {} {} {}",
                                app.name,
                                group.title,
                                shortcut.title,
                                shortcut.display.join(" "),
                                shortcut.description.as_deref().unwrap_or_default()
                            );
                            if !matches_query(&haystack, &self.query) {
                                return None;
                            }
                            Some(Row {
                                title: shortcut.title.clone(),
                                detail: shortcut.description.clone(),
                                chips: shortcut.display.clone(),
                                conflict: index.has_conflict(
                                    "app",
                                    &app.id,
                                    &shortcut.title,
                                    &shortcut.keys,
                                ) || index.has_conflict(
                                    &app.source,
                                    &app.id,
                                    &shortcut.title,
                                    &shortcut.keys,
                                ),
                            })
                        })
                        .collect();
                    if !rows.is_empty() {
                        groups.push(ViewGroup {
                            title: if all_apps {
                                format!("{} · {}", app.name, group.title)
                            } else {
                                group.title.clone()
                            },
                            rows,
                        });
                    }
                }
            }
            sections.push(Section {
                scope: Scope::ThisApp,
                title: if all_apps {
                    "All apps".into()
                } else {
                    "This app".into()
                },
                groups,
            });
        }
        if self.scope == Scope::All || self.scope == Scope::Arcade {
            let mut groups: Vec<ViewGroup> = Vec::new();
            for shortcut in &index.arcade {
                let haystack = format!(
                    "{} {} {}",
                    shortcut.name,
                    shortcut.title,
                    shortcut.display.join(" ")
                );
                if !matches_query(&haystack, &self.query) {
                    continue;
                }
                let row = Row {
                    title: shortcut.title.clone(),
                    detail: None,
                    chips: shortcut.display.clone(),
                    conflict: index.has_conflict(
                        "arcade",
                        &shortcut.app,
                        &shortcut.title,
                        &shortcut.keys,
                    ),
                };
                if let Some(group) = groups.iter_mut().find(|group| group.title == shortcut.name) {
                    group.rows.push(row);
                } else {
                    groups.push(ViewGroup {
                        title: shortcut.name.clone(),
                        rows: vec![row],
                    });
                }
            }
            sections.push(Section {
                scope: Scope::Arcade,
                title: "Arcade".into(),
                groups,
            });
        }
        if self.scope == Scope::All || self.scope == Scope::WindowManager {
            let mut groups = Vec::new();
            if let Some(wm) = &index.wm {
                for group in &wm.groups {
                    let rows: Vec<_> = group
                        .shortcuts
                        .iter()
                        .filter_map(|shortcut| {
                            let haystack = format!(
                                "{} {} {} {} {}",
                                group.title,
                                shortcut.label,
                                shortcut.display,
                                shortcut.detail.as_deref().unwrap_or_default(),
                                shortcut.submap.as_deref().unwrap_or_default()
                            );
                            if !matches_query(&haystack, &self.query) {
                                return None;
                            }
                            let mut detail = shortcut.detail.clone().unwrap_or_default();
                            let state = match shortcut.state.as_str() {
                                "configuredOnly" => "Configured, not active",
                                "runtimeOnly" => "Runtime binding",
                                _ => "",
                            };
                            for extra in [state, shortcut.submap.as_deref().unwrap_or_default()] {
                                if !extra.is_empty() {
                                    if !detail.is_empty() {
                                        detail.push_str(" · ");
                                    }
                                    detail.push_str(extra);
                                }
                            }
                            Some(Row {
                                title: shortcut.label.clone(),
                                detail: (!detail.is_empty()).then_some(detail),
                                chips: vec![shortcut.display.clone()],
                                conflict: index.has_conflict(
                                    "wm",
                                    &wm.name,
                                    &shortcut.label,
                                    std::slice::from_ref(&shortcut.chord),
                                ),
                            })
                        })
                        .collect();
                    if !rows.is_empty() {
                        groups.push(ViewGroup {
                            title: group.title.clone(),
                            rows,
                        });
                    }
                }
            }
            sections.push(Section {
                scope: Scope::WindowManager,
                title: "Window manager".into(),
                groups,
            });
        }
        sections
    }
}

/// Word-wise case-insensitive substring match, then ordered subsequence for
/// compact queries such as "nwtab" or "csf" (key displays are included).
pub fn matches_query(haystack: &str, query: &str) -> bool {
    let haystack = haystack.to_lowercase();
    query.to_lowercase().split_whitespace().all(|word| {
        if haystack.contains(word) {
            return true;
        }
        if word.chars().count() < 3 {
            return false;
        }
        let mut chars = haystack.chars();
        word.chars()
            .all(|wanted| chars.any(|candidate| candidate == wanted))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn model(app: Option<usize>) -> Model {
        Model::new(
            Some(Index::parse(include_bytes!("../tests/fixtures/shortcuts-index.json")).unwrap()),
            app,
        )
    }

    #[test]
    fn scopes_cycle_in_both_directions() {
        let mut scope = Scope::ThisApp;
        for expected in [
            Scope::Arcade,
            Scope::WindowManager,
            Scope::All,
            Scope::ThisApp,
        ] {
            scope = scope.next(false);
            assert_eq!(scope, expected);
        }
        assert_eq!(scope.next(true), Scope::All);
    }

    #[test]
    fn search_matches_titles_and_key_displays_across_everything() {
        let mut model = model(Some(0));
        model.scope = Scope::ThisApp;
        model.type_text("clipboard");
        assert_eq!(model.scope, Scope::All);
        assert!(model
            .sections()
            .iter()
            .flat_map(|s| &s.groups)
            .flat_map(|g| &g.rows)
            .any(|r| r.title == "Paste clipboard"));
        model.clear_query();
        model.type_text("ctrl+alt+v");
        assert_eq!(
            model
                .sections()
                .iter()
                .flat_map(|s| &s.groups)
                .flat_map(|g| &g.rows)
                .filter(|r| r.conflict)
                .count(),
            2
        );
        assert!(matches_query("Open a new tab Ctrl+T", "NWTab"));
        assert!(!matches_query("Open a new tab", "xyz"));
        assert!(matches_query("Änderung öffnen", "Änderung"));
    }

    #[test]
    fn arcade_in_app_keys_are_only_in_this_app() {
        let model = model(Some(2));
        let sections = model.sections();
        assert!(sections[0]
            .groups
            .iter()
            .flat_map(|g| &g.rows)
            .any(|r| r.title == "Open selected file"));
        assert!(!sections[1]
            .groups
            .iter()
            .flat_map(|g| &g.rows)
            .any(|r| r.title == "Open selected file"));
    }

    #[test]
    fn unknown_app_picker_and_missing_index() {
        let mut model = model(None);
        assert!(model.picking);
        model.type_text("firefox");
        assert_eq!(model.picker_candidates(), [0]);
        model.pick(None);
        assert_eq!(model.title(), "Firefox");
        assert!(!model.picking);
        assert!(model.query.is_empty());
        let missing = Model::new(None, None);
        assert_eq!(missing.message(), Some(MISSING_MESSAGE));
        assert!(missing.sections().is_empty());
    }
}

//! Local crash reports (TOOLS.md §Crash reports, ARCADE_APP.md RT-4). Every app, Tools included, writes a report for
//! a panic or a fatal signal into `arcade/<app>/crashes/` through Link (`runtime::install_crash_reports`). Tools
//! lists the ones not dismissed yet, shows the full text, and only from there offers to copy it or to open a GitHub
//! issue pre-filled with it. Nothing is ever sent anywhere by itself.
use crate::paths::Paths;
use crate::release::Os;
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

/// The largest report Tools reads; Link's reports are a few kB (header, message and backtrace).
const MAX_REPORT: u64 = 256 * 1024;
/// GitHub refuses longer `issues/new` URLs; the body is cut to fit and says so.
const MAX_ISSUE_URL: usize = 8000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CrashSummary {
    pub app: String,
    /// The report's file name in the app's crash folder: its identity for reading and dismissing.
    pub file: String,
    /// Seconds since the epoch, from the file name Link gives it.
    pub created: u64,
    /// `panic` or `signal`.
    pub kind: String,
    /// The report's `message:` or `signal:` line, for the notice.
    pub message: String,
}

#[derive(Default, Serialize, Deserialize)]
struct Dismissed {
    #[serde(default)]
    schema: u32,
    #[serde(default)]
    apps: BTreeMap<String, BTreeSet<String>>,
}

fn crash_dir(paths: &Paths, app: &str) -> PathBuf {
    paths.app_dirs(app, Os::current()).data.join("crashes")
}

fn dismissed_file(paths: &Paths) -> PathBuf {
    paths.state().join("crashes-dismissed.json")
}

fn load_dismissed(paths: &Paths) -> Dismissed {
    std::fs::read(dismissed_file(paths))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// A report file name as Link writes it (`<seconds>-<pid>-<kind>.crash`): nothing that could leave the folder.
fn valid_name(file: &str) -> bool {
    file.ends_with(".crash")
        && !file.starts_with('.')
        && file
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.')
}

/// The reports of `apps` not dismissed yet, newest first.
pub fn list(paths: &Paths, apps: &[String]) -> Vec<CrashSummary> {
    let dismissed = load_dismissed(paths);
    let mut out = Vec::new();
    for app in apps {
        let seen = dismissed.apps.get(app);
        for path in arcade_link::runtime::crash_reports(&crash_dir(paths, app)) {
            let Some(file) = path
                .file_name()
                .and_then(|n| n.to_str())
                .filter(|n| valid_name(n))
            else {
                continue;
            };
            if seen.is_some_and(|seen| seen.contains(file)) {
                continue;
            }
            let created = file
                .split('-')
                .next()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            let kind = file
                .trim_end_matches(".crash")
                .rsplit('-')
                .next()
                .unwrap_or("crash")
                .to_owned();
            let message = read_bounded(&path)
                .ok()
                .and_then(|text| {
                    text.lines()
                        .find_map(|line| {
                            line.strip_prefix("message: ")
                                .or_else(|| line.strip_prefix("signal: "))
                        })
                        .map(|line| line.chars().take(200).collect())
                })
                .unwrap_or_default();
            out.push(CrashSummary {
                app: app.clone(),
                file: file.to_owned(),
                created,
                kind,
                message,
            });
        }
    }
    out.sort_by(|a, b| b.created.cmp(&a.created).then_with(|| a.app.cmp(&b.app)));
    out
}

fn read_bounded(path: &std::path::Path) -> std::io::Result<String> {
    use std::io::Read;
    let mut text = String::new();
    std::fs::File::open(path)?
        .take(MAX_REPORT)
        .read_to_string(&mut text)?;
    Ok(text)
}

/// The full text of one report.
pub fn read(paths: &Paths, app: &str, file: &str) -> Result<String> {
    if !valid_name(file) {
        return Err(Error::new("crash", "That is not a crash report."));
    }
    read_bounded(&crash_dir(paths, app).join(file))
        .map_err(|e| Error::new("crash", format!("Couldn't read the crash report: {e}")))
}

/// Hides a report from the list; the file stays until Link's own pruning (the newest 20 per app) removes it.
pub fn dismiss(paths: &Paths, app: &str, file: &str) -> Result<()> {
    if !valid_name(file) {
        return Err(Error::new("crash", "That is not a crash report."));
    }
    let mut dismissed = load_dismissed(paths);
    dismissed.schema = 1;
    dismissed
        .apps
        .entry(app.to_owned())
        .or_default()
        .insert(file.to_owned());
    // Forget reports that no longer exist, so the file stays small.
    for (app, files) in dismissed.apps.iter_mut() {
        let dir = crash_dir(paths, app);
        files.retain(|file| dir.join(file).exists());
    }
    dismissed.apps.retain(|_, files| !files.is_empty());
    let bytes =
        serde_json::to_vec_pretty(&dismissed).map_err(|e| Error::new("crash", e.to_string()))?;
    std::fs::create_dir_all(paths.state())?;
    arcade_link::paths::write_atomic(&dismissed_file(paths), &bytes, true)?;
    Ok(())
}

fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 3);
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// A `github.com/<repo>/issues/new` link pre-filled with the report. `repo` is a catalog repository (`owner/name`),
/// already checked against the owner allowlist. The report is cut at a line boundary to keep the link within
/// GitHub's limit, and the issue then says so.
pub fn issue_url(repo: &str, app_name: &str, report: &str) -> String {
    let message = report
        .lines()
        .find_map(|line| {
            line.strip_prefix("message: ")
                .or_else(|| line.strip_prefix("signal: "))
        })
        .unwrap_or("crash");
    let title: String = format!("{app_name} crashed: {message}")
        .chars()
        .take(120)
        .collect();
    let base = format!(
        "https://github.com/{repo}/issues/new?title={}&body=",
        encode(&title)
    );
    let intro = format!(
        "{app_name} wrote this crash report (from Arcade Tools, reviewed before sending).\n\n```\n"
    );
    let cut_note = "\n[report shortened to fit; attach the full file if asked]";
    let mut lines: Vec<&str> = report.lines().collect();
    let mut cut = false;
    loop {
        let body = format!(
            "{intro}{}{}\n```\n",
            lines.join("\n"),
            if cut { cut_note } else { "" }
        );
        let url = format!("{base}{}", encode(&body));
        if url.len() <= MAX_ISSUE_URL || lines.is_empty() {
            return url;
        }
        lines.pop();
        cut = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths() -> (tempfile::TempDir, Paths) {
        let root = tempfile::tempdir().unwrap();
        let paths = Paths::under(root.path());
        (root, paths)
    }

    fn write(paths: &Paths, app: &str, file: &str, text: &str) {
        let dir = crash_dir(paths, app);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(file), text).unwrap();
    }

    #[test]
    fn lists_reports_newest_first_until_dismissed() {
        let (_root, paths) = paths();
        write(
            &paths,
            "arcade.box",
            "00001794190000-41-panic.crash",
            "app: Arcade Box\nkind: panic\nmessage: index out of bounds\n",
        );
        write(
            &paths,
            "arcade.tools",
            "00001794190500-7-signal.crash",
            "app: Arcade Tools\nkind: signal\nsignal: SIGSEGV\n",
        );
        write(&paths, "arcade.box", "notes.txt", "not a report");
        let apps = vec!["arcade.box".to_owned(), "arcade.tools".to_owned()];
        let list = list(&paths, &apps);
        assert_eq!(list.len(), 2);
        assert_eq!(
            (
                list[0].app.as_str(),
                list[0].kind.as_str(),
                list[0].message.as_str()
            ),
            ("arcade.tools", "signal", "SIGSEGV")
        );
        assert_eq!(
            (list[1].created, list[1].message.as_str()),
            (1794190000, "index out of bounds")
        );
        dismiss(&paths, "arcade.box", &list[1].file).unwrap();
        let after = super::list(&paths, &apps);
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].app, "arcade.tools");
    }

    #[test]
    fn reads_only_report_files_in_the_app_folder() {
        let (_root, paths) = paths();
        write(
            &paths,
            "arcade.box",
            "00001794190000-41-panic.crash",
            "app: Arcade Box\n",
        );
        assert_eq!(
            read(&paths, "arcade.box", "00001794190000-41-panic.crash").unwrap(),
            "app: Arcade Box\n"
        );
        for bad in [
            "../../etc/passwd",
            "../x.crash",
            "/etc/x.crash",
            ".crash",
            "a b.crash",
            "x.txt",
        ] {
            assert!(read(&paths, "arcade.box", bad).is_err(), "{bad}");
            assert!(dismiss(&paths, "arcade.box", bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_issue_link_carries_the_report_within_github_limits() {
        let report = "app: Arcade Box\nkind: panic\nmessage: boom & bust\n\nbacktrace:\n   0: arcade_box::main\n";
        let url = issue_url("qa-p1/Arcade-box", "Arcade Box", report);
        assert!(url.starts_with("https://github.com/qa-p1/Arcade-box/issues/new?title=Arcade%20Box%20crashed%3A%20boom%20%26%20bust&body="), "{url}");
        assert!(url.contains("arcade_box%3A%3Amain"));
        let long: String = (0..5000).map(|i| format!("  {i}: frame\n")).collect();
        let url = issue_url(
            "qa-p1/Arcade-box",
            "Arcade Box",
            &format!("message: deep\n{long}"),
        );
        assert!(url.len() <= MAX_ISSUE_URL, "{}", url.len());
        assert!(url.contains("shortened%20to%20fit"));
    }
}

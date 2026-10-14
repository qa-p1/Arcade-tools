use arcade_tools_core::manager::Manager;
use arcade_tools_core::paths::Paths;
use arcade_tools_core::self_update::{SelfRollbackPlan, SelfUninstallPlan, SelfUpdatePlan};
use arcade_tools_core::source::Source;
use arcade_tools_core::{Error, Result};
use std::path::Path;
use std::sync::atomic::AtomicBool;

pub fn launch(paths: &Paths, operation: &str, plan: &Path) -> Result<()> {
    let directory = paths.state().join("helpers");
    paths.guard(&directory)?;
    arcade_link::paths::ensure_private_dir(&directory)?;
    let copy = tempfile::Builder::new()
        .prefix("tools-helper-")
        .tempdir_in(&directory)?;
    let executable = copy.path().join(if cfg!(windows) {
        "arcade-tools.exe"
    } else {
        "arcade-tools"
    });
    std::fs::copy(std::env::current_exe()?, &executable)?;
    // A separate executable is required on Windows, where a running binary
    // cannot be replaced. This helper is never used as an autostart target.
    arcade_tools_core::platform::spawn_detached(
        paths,
        &executable,
        &[operation.into(), plan.to_string_lossy().into_owned()],
    )?;
    // The helper outlives this process and runs from the copy, so keep it.
    let _ = copy.keep();
    Ok(())
}

pub fn run(args: &[String]) -> Option<Result<()>> {
    let operation = args.get(1)?.as_str();
    if !matches!(
        operation,
        "--finish-self-update" | "--finish-self-rollback" | "--finish-self-uninstall"
    ) {
        return None;
    }
    Some((|| {
        let paths = Paths::discover()?;
        let file = Path::new(
            args.get(2)
                .ok_or_else(|| Error::new("usage", "Provide the staged plan."))?,
        );
        paths.guard(file)?;
        if !file.starts_with(paths.state().join("self-update")) {
            return Err(Error::new("paths", "Helper plan escaped Tools state."));
        }
        #[cfg(debug_assertions)]
        let source = if paths.isolated {
            std::env::var("ARCADE_TOOLS_TEST_SOURCE")
                .ok()
                .map(|url| Source::test_server(&url))
                .transpose()?
                .unwrap_or(Source::github()?)
        } else {
            Source::github()?
        };
        #[cfg(not(debug_assertions))]
        let source = Source::github()?;
        let manager = Manager::new(paths, source);
        let cancel = AtomicBool::new(false);
        if operation == "--finish-self-update" {
            let plan = SelfUpdatePlan::read(&manager.paths, file)?;
            manager.finish_self_update(&plan, &cancel, &mut |_| {})?;
        } else if operation == "--finish-self-rollback" {
            let plan: SelfRollbackPlan = serde_json::from_slice(
                &arcade_tools_core::catalog::read_bounded(file, 1024 * 1024)?,
            )?;
            manager.finish_self_rollback(&plan, &cancel, &mut |_| {})?;
        } else {
            let plan: SelfUninstallPlan = serde_json::from_slice(
                &arcade_tools_core::catalog::read_bounded(file, 1024 * 1024)?,
            )?;
            manager.finish_self_uninstall(&plan, &cancel)?;
        }
        let _ = std::fs::remove_file(file);
        Ok(())
    })())
}

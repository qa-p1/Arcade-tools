//! Read the real config/runtime without modifying either. JSON on stdout;
//! diagnostics on stderr. Writing the generated cache is explicitly optional.

use arcade_shortcuts::{build_index, labels, wm, BindState, BuildInputs};
use std::path::PathBuf;
use std::time::Instant;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut inputs = BuildInputs::default();
    let mut output = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let value = args.next().ok_or("expected a path after each option")?;
        match arg.as_str() {
            "--catalog" => inputs.catalog_dir = Some(value.into()),
            "--registry" => inputs.registry_dir = Some(value.into()),
            "--docs" => inputs.arcade_docs.push(value.into()),
            "--labels" => inputs.user_labels = Some(value.into()),
            "--output" => output = Some(PathBuf::from(value)),
            _ => {
                return Err(format!(
                    "unknown option {arg}; use --catalog, --registry, --docs, --labels, --output"
                )
                .into())
            }
        }
    }
    let start = Instant::now();
    let user_labels = inputs
        .user_labels
        .as_deref()
        .map(labels::load_user_labels)
        .transpose()?
        .unwrap_or_default();
    let report = wm::read_machine(&user_labels);
    let desktop = report.name.clone();
    let read_elapsed = start.elapsed();
    let active = report
        .binds
        .iter()
        .filter(|b| b.state == BindState::Active)
        .count();
    let configured = report
        .binds
        .iter()
        .filter(|b| b.state == BindState::ConfiguredOnly)
        .count();
    let runtime = report
        .binds
        .iter()
        .filter(|b| b.state == BindState::RuntimeOnly)
        .count();
    let files = report.files.len();
    inputs.wm = Some(report);
    let index = build_index(&inputs);
    if let Some(output) = output {
        index.write_index(&output)?;
    }
    let total = start.elapsed();
    eprintln!("{desktop}: active={active}, configured-only={configured}, runtime-only={runtime}, files={files}; apps={}; wm displayed={}; source read={:.3}ms, complete rebuild={:.3}ms",
        index.apps.len(), index.wm.groups.iter().map(|g| g.shortcuts.len()).sum::<usize>(), read_elapsed.as_secs_f64() * 1000.0, total.as_secs_f64() * 1000.0);
    for warning in &index.wm.warnings {
        eprintln!("Warning: {warning}");
    }
    serde_json::to_writer_pretty(std::io::stdout().lock(), &index)?;
    println!();
    Ok(())
}

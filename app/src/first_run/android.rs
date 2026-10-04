//! Android executes the shared preparation plan in-process instead of spawning shell tools.

mod sources;

use std::{
    fs::{self, OpenOptions},
    io::Write,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

use anyhow::{Context, Result, bail};

use super::{
    plan::{self, Action},
    prepare, runner, stamp,
    status::{Phase, Status},
};
use crate::{android::bridge, install_layout::InstallLayout};

// Use the same cancellation type as the desktop executor and publisher.
use super::runner::Cancelled;

pub(crate) fn cancelled() -> anyhow::Error {
    Cancelled.into()
}

pub(crate) fn is_current(layout: &InstallLayout) -> Result<bool> {
    Ok(prepare::selection(layout)?.1.is_current())
}

/// Called on the Java bootstrap worker before any NativeActivity window exists.
pub(crate) fn bootstrap(cancel: &AtomicBool) -> Result<bool> {
    let layout = InstallLayout::discover()?;
    fs::create_dir_all(layout.log_dir())?;
    let mut report = super::reporter(&layout, |status| {
        let text = match (status.downloaded, status.download_total) {
            (Some(received), Some(total)) => format!(
                "{}: {} / {} MB",
                status.label,
                received / 1_000_000,
                total / 1_000_000
            ),
            _ => status.label.clone(),
        };
        bridge::progress(&text);
    });
    if is_current(&layout)? {
        return Ok(true);
    }
    if !super::consent_recorded(&layout) {
        report(Status::new(
            Phase::AwaitingConsent,
            0,
            0,
            "Waiting for consent",
        ));
        bridge::strings_call("requestConsent", super::TITLE, super::CONSENT_BODY)?;
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Ok(false);
            }
            match bridge::consent_state()? {
                1 => break,
                -1 => return Ok(false),
                _ => std::thread::sleep(Duration::from_millis(100)),
            }
        }
        super::record_consent(&layout)?;
    }
    let result = compile(&layout, cancel, &mut report);
    match result {
        Ok(()) => {
            report(Status::new(Phase::Done, 0, 0, "Ready"));
            Ok(true)
        }
        Err(error) if error.is::<Cancelled>() => Ok(false),
        Err(error) => {
            report(Status::failed("Setup failed", &format!("{error:#}")));
            Err(error)
        }
    }
}

fn compile(
    layout: &InstallLayout,
    cancel: &AtomicBool,
    report: &mut dyn FnMut(Status),
) -> Result<()> {
    let (steps, selection) = prepare::selection(layout)?;
    let workspace = layout.prepare_workspace();
    let prepared = layout.prepared_assets_dir();
    runner::stage_kit(&layout.prep_kit(), &workspace)?;
    if selection.needs_pack {
        super::download::fetch_archive(&workspace, cancel, |received, total| {
            report(Status::downloading(received, total));
        })?;
    }
    let staged = workspace.join(plan::COMPILED);
    if staged.exists() {
        fs::remove_dir_all(&staged)?;
    }
    fs::create_dir_all(&staged)?;
    if plan::carriers_present(&prepared) {
        runner::seed(&prepared, &staged)?;
    }
    for output in &selection.outputs {
        runner::clear_output(&staged, output);
    }
    let stale: Vec<_> = steps
        .into_iter()
        .zip(&selection.run)
        .filter_map(|(step, run)| run.then_some(step))
        .collect();
    let mut log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(layout.log_dir().join("first-run.log"))?;
    let total = stale.len();
    let skipped = runner::execute_steps(
        &stale,
        |step| {
            if cancel.load(Ordering::Relaxed) {
                return Err(Cancelled.into());
            }
            writeln!(log, "{}", step.label)?;
            let result = match &step.action {
                Action::Script("fetch-vanilla-assets") => sources::unpack_pack(&workspace, cancel),
                Action::Script(name) => Err(anyhow::anyhow!(
                    "unsupported Android preparation step {name}"
                )),
                Action::Assetc(args) => {
                    let absolute = absolute_arguments(&workspace, args)?;
                    asset_compiler::run_args(absolute).map_err(|error| anyhow::anyhow!("{error}"))
                }
            };
            if let Err(error) = &result {
                let _ = writeln!(log, "{error:#}");
            }
            result
        },
        |index, step| report(Status::new(Phase::Running, index + 1, total, step.label)),
    )?;
    for label in skipped {
        writeln!(log, "optional step skipped: {label}")?;
    }
    if !plan::carriers_present(&staged) {
        bail!(
            "required carriers are missing under {}; reopen the app to rebuild them",
            staged.display()
        );
    }
    stamp::write(&staged, &selection.identities)?;
    runner::publish_if_active(&staged, &prepared, cancel)?;
    let _ = fs::remove_dir_all(workspace.join(".local/assets/bedrock-samples"));
    super::download::prune(&workspace);
    Ok(())
}

/// Every argument after a plan flag is a workspace path; resolve it without changing JVM cwd.
fn absolute_arguments(workspace: &std::path::Path, args: &[String]) -> Result<Vec<String>> {
    let (command, flags) = args.split_first().context("empty asset compiler command")?;
    if flags.len() % 2 != 0 {
        bail!("preparation command has an incomplete flag");
    }
    let mut output = vec!["assetc".to_owned(), command.clone()];
    for pair in flags.chunks_exact(2) {
        if !pair[0].starts_with("--") {
            bail!("invalid preparation flag {}", pair[0]);
        }
        output.push(pair[0].clone());
        output.push(workspace.join(&pair[1]).to_string_lossy().into_owned());
    }
    Ok(output)
}

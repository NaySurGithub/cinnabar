//! The checked carrier compiler, shared by the CLI and native mobile setup.

#[path = "command/cli.rs"]
mod cli;
mod command;

use clap::Parser;

/// Parses and compiles one carrier with the same checked inputs as the desktop CLI.
pub fn run_args<I, T>(args: I) -> Result<(), Box<dyn std::error::Error>>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    command::run(cli::Cli::try_parse_from(args)?.command)
}

/// Prepares native carriers, reporting JSON lines without redirecting process output.
pub fn run_prepare_args<I, T>(
    args: I,
    line: impl Fn(&str) + Sync,
    cancelled: impl Fn() -> bool + Sync,
) -> Result<(), Box<dyn std::error::Error>>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    command::run_prepare(cli::Cli::try_parse_from(args)?.command, &line, &cancelled)
}

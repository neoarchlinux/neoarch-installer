pub mod error;
pub mod installer;
pub mod partitioner;
pub mod ui;

use std::process::Command;

use error::{Error, Result};

use crate::{installer::Installer, partitioner::Partitioner};

fn run() -> Result<()> {
    ui::msgbox(
        "NeoArch Installer",
        "\nWelcome to the NeoArch Installer.\n\
        \n\
         This tool will help you:\n\
         - set up partitions for your new system\n\
         - install NeoArch Linux according to your config\n\
         \n\
         Use arrow keys to navigate, ENTER to select, TAB to switch buttons.",
    )?;

    'top: loop {
        let mut partitioner = Partitioner::new()?;

        let ptype = loop {
            match ui::menu(
                "Partitioning",
                "Choose the partitioning type",
                &[
                    ("simple", "one drive, full drive usage (recommended)"),
                    ("manual", "partition on your own using the shell (advanced)"),
                ],
            ) {
                Ok(s) => break s,
                Err(Error::Cancelled) => {
                    if ui::yesno("Exit Installer", "Exit the NeoArch installer?").unwrap_or(false) {
                        return Err(Error::Cancelled);
                    }
                }
                Err(e) => return Err(e),
            }
        };

        match match ptype.as_str() {
            "simple" => partitioner.run_simple_partitioning(),
            "manual" => partitioner.run_manual_partitioning(),
            other => unimplemented!("partitioning {other}"),
        } {
            Ok(()) => {}
            Err(Error::Cancelled) => continue 'top,
            Err(e) => return Err(e),
        }

        let Some(plan) = partitioner.current_plan else {
            continue 'top;
        };

        match Installer::new(plan).install() {
            Ok(()) => return Ok(()),
            Err(Error::Cancelled) => continue 'top,
            Err(e) => return Err(e),
        }
    }
}

fn main() {
    let result = run();

    match result {
        Ok(()) | Err(error::Error::Cancelled) => {}
        Err(error::Error::InstallCommandFailed {
            cmd,
            stdout,
            stderr,
        }) => {
            let mut msg = format!("Command failed:\n  {cmd}");
            if !stdout.trim().is_empty() {
                msg.push_str(&format!("\n\nStdout:\n{stdout}"));
            }
            if !stderr.trim().is_empty() {
                msg.push_str(&format!("\n\nStderr:\n{stderr}"));
            }
            let _ = ui::msgbox("Installation Failed", &msg);
        }
        Err(e) => {
            let _ = ui::msgbox("Error", &e.to_string());
        }
    }

    ui::clear();

    let _ = Command::new("reset").status();
}

#[macro_export]
macro_rules! cmdp {
    ($cmd:expr $(, $arg:expr)* $(,)?) => {
        (
            ::std::string::ToString::to_string(&$cmd),
            vec![
                $(
                    ::std::string::ToString::to_string(&$arg)
                ),*
            ]
        )
    };
}

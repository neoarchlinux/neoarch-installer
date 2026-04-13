pub mod error;
pub mod installer;
pub mod partitioner;
pub mod ui;

use std::process::Command;

use error::Result;

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

    let mut partitioner = Partitioner::new()?;

    match ui::menu(
        "Partitioning",
        "Choose the paritioning type",
        &[
            ("simple", "one drive, full drive usage (recommended)"),
            ("manual", "partition on your own using the shell (advanced)"),
        ],
    )?
    .as_str()
    {
        "simple" => partitioner.run_simple_partitioning(),
        "manual" => partitioner.run_manual_partitioning(),
        other => unimplemented!("partitioning {other}"),
    }?;

    let Some(plan) = partitioner.current_plan else {
        todo!()
    };

    let installer = Installer::new(plan);

    installer.install()
}

fn main() -> Result<()> {
    let result = run();
    let _ = Command::new("reset").status();
    std::thread::sleep(std::time::Duration::from_secs(1));
    let _ = Command::new("reset").status();
    result
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

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Dialog command failed: {0}")]
    Dialog(String),

    #[error("No disks found on the system")]
    NoDisks,

    #[error("User cancelled")]
    Cancelled,

    #[error("Input/Output error: {0}")]
    IO(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Shell exited with error")]
    ShellError,

    #[error("No root filesystem mounted at /mnt")]
    ValidationMissingRoot,

    #[error("Partition plan missing")]
    PartitionPlanMissing,

    #[error("An error occured during the installation. Latest command: {0}")]
    InstallError(String),
}

pub type Result<T> = std::result::Result<T, Error>;

use crate::error::Result;
use crate::{cmdp, ui};

use std::collections::HashMap;
use std::process::Command;

use serde::Deserialize;

static SIMPLE_PARTITIONING_SUBVOLUMES: &[(&str, &str)] = &[
    ("@", "/"),
    ("@home", "/home"),
    ("@var", "/var"),
    ("@log", "/var/log"),
    ("@cache", "/var/cache"),
    ("@snapshots", "/.snapshots"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Filesystem {
    Btrfs,
    Ext4,
    Xfs,
    Fat,
    Vfat,
}

impl Filesystem {
    pub fn as_str(&self) -> &'static str {
        match self {
            Filesystem::Btrfs => "btrfs",
            Filesystem::Ext4 => "ext4",
            Filesystem::Xfs => "xfs",
            Filesystem::Fat => "fat",
            Filesystem::Vfat => "vfat",
        }
    }
}

impl std::fmt::Display for Filesystem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl TryFrom<String> for Filesystem {
    type Error = String;

    fn try_from(value: String) -> std::result::Result<Self, Self::Error> {
        match value.as_str() {
            "btrfs" => Ok(Filesystem::Btrfs),
            "ext4" => Ok(Filesystem::Ext4),
            "xfs" => Ok(Filesystem::Xfs),
            "fat" => Ok(Filesystem::Fat),
            "vfat" => Ok(Filesystem::Vfat),
            f => Err(format!("Unknown filesystem: '{f}'")),
        }
    }
}

#[derive(Deserialize, Debug)]
pub struct BlockDevice {
    pub name: String,
    pub path: String,
    pub model: Option<String>,
    #[serde(rename = "type")]
    pub dev_type: String,
    pub mountpoints: Option<Vec<String>>,
    pub children: Option<Vec<BlockDevice>>,
    pub size: String,
}

pub fn lsblk() -> Result<Vec<BlockDevice>> {
    #[derive(Deserialize, Debug)]
    struct Lsblk {
        blockdevices: Vec<BlockDevice>,
    }

    let output = Command::new("lsblk").args(["--json", "-O"]).output()?;
    let parsed: Lsblk = serde_json::from_slice(&output.stdout)?;
    Ok(parsed.blockdevices)
}

pub fn detect_manual_partitioning() -> Result<PartitionPlan> {
    #[derive(Deserialize, Debug)]
    struct FindmntFs {
        source: String,
        target: String,
        fstype: String,
        options: Option<String>,
        children: Option<Vec<FindmntFs>>,
    }

    #[derive(Deserialize, Debug)]
    struct Findmnt {
        filesystems: Vec<FindmntFs>,
    }

    let output = Command::new("findmnt")
        .args([
            "--json",
            "-n",
            "-o",
            "SOURCE,TARGET,FSTYPE,OPTIONS",
            "-R",
            "/mnt",
        ])
        .output()?;

    let parsed: Findmnt = serde_json::from_slice(&output.stdout)?;

    fn process_fs(
        fs: FindmntFs,
        partitions_map: &mut std::collections::HashMap<String, Partition>,
        subvolumes: &mut Vec<BtrfsSubvolume>,
    ) {
        let source = fs
            .source
            .split('[')
            .next()
            .unwrap_or(&fs.source)
            .to_string();

        let target = fs
            .target
            .trim_start_matches(|c: char| !c.is_ascii_alphanumeric() && c != '/');

        let mountpoint = match target.strip_prefix("/mnt") {
            Some("") | None => "/",
            Some(other) => other,
        }
        .to_string();

        let filesystem = match Filesystem::try_from(fs.fstype.clone()) {
            Ok(fs) => fs,
            Err(_) => return,
        };

        let opts = fs.options.unwrap_or_default();

        let subvol_name = if filesystem == Filesystem::Btrfs {
            opts.split(',')
                .find_map(|opt| opt.strip_prefix("subvol="))
                .map(|s| s.trim_start_matches('/').to_string())
        } else {
            None
        };

        partitions_map
            .entry(source.clone())
            .or_insert_with(|| Partition {
                device_path: source.clone(),
                filesystem,
                mountpoint: None,
            });

        if let Some(subvol) = subvol_name {
            subvolumes.push(BtrfsSubvolume {
                name: subvol,
                mountpoint: mountpoint.clone(),
            });
        } else {
            if let Some(part) = partitions_map.get_mut(&source) {
                part.mountpoint = Some(mountpoint.clone());
            }
        }

        if let Some(children) = fs.children {
            for child in children {
                process_fs(child, partitions_map, subvolumes);
            }
        }
    }

    let mut partitions_map = HashMap::<String, Partition>::new();
    let mut subvolumes = Vec::<BtrfsSubvolume>::new();

    for fs in parsed.filesystems {
        process_fs(fs, &mut partitions_map, &mut subvolumes);
    }

    Ok(PartitionPlan::Manual {
        partitions: partitions_map.into_values().collect(),
        btrfs_subvolumes: subvolumes,
    })
}

#[derive(Debug, Clone)]
pub struct Partition {
    pub device_path: String,
    pub filesystem: Filesystem,
    pub mountpoint: Option<String>,
}

#[derive(Debug, Clone)]
pub struct BtrfsSubvolume {
    pub name: String,
    pub mountpoint: String,
}

impl<S> From<(S, S)> for BtrfsSubvolume
where
    S: ToString,
{
    fn from(v: (S, S)) -> Self {
        BtrfsSubvolume {
            name: v.0.to_string(),
            mountpoint: v.1.to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum PartitionPlan {
    Simple {
        device: String,
    },
    Manual {
        partitions: Vec<Partition>,
        btrfs_subvolumes: Vec<BtrfsSubvolume>,
    },
}

pub struct Partitioner {
    devices: Vec<BlockDevice>,
    pub current_plan: Option<PartitionPlan>,
}

impl Partitioner {
    pub fn new() -> Result<Self> {
        Ok(Self {
            devices: lsblk()?,
            current_plan: None,
        })
    }

    pub fn refresh_devices(&mut self) -> Result<()> {
        self.devices = lsblk()?;
        Ok(())
    }

    pub fn get_disks(&self) -> Vec<&BlockDevice> {
        self.devices
            .iter()
            .filter(|d| d.dev_type == "disk")
            .filter(|d| {
                !d.path.starts_with("/dev/loop")
                    && !d.path.starts_with("/dev/ram")
                    && !d.path.starts_with("/dev/zram")
            })
            .collect()
    }

    fn partition_path(disk: &str, num: u32) -> String {
        if disk.contains("nvme") || disk.contains("mmcblk") {
            format!("{}p{}", disk, num)
        } else {
            format!("{}{}", disk, num)
        }
    }

    pub fn run_simple_partitioning(&mut self) -> Result<()> {
        let disks = self.get_disks();
        if disks.is_empty() {
            ui::msgbox("Error", "No suitable disks found!")?;
            return Ok(());
        }

        let items: Vec<(String, String)> = disks
            .iter()
            .map(|d| {
                let label = format!(
                    "{} ({}{})",
                    d.path.clone(),
                    d.size.clone(),
                    d.model
                        .clone()
                        .map_or(String::new(), |m| m.trim().to_string())
                );

                (d.path.clone(), label)
            })
            .collect();

        let selected = ui::menu(
            "Simple Partitioning",
            "Select disk for NEOARCH installation:",
            &items,
        )?;

        let disk = disks.iter().find(|d| d.path == selected).unwrap();

        let confirm = format!(
            "WARNING: This will ERASE ALL DATA on {} ({})\n\n\
             Layout to create:\n\
             - {} (1GB FAT32) → /boot/efi\n\
             - {} (Btrfs) with subvolumes:\n\
             {}\n\n\
             Proceed with destruction?",
            disk.path,
            disk.model.clone().unwrap_or("MODEL UNKNOWN".to_string()),
            Self::partition_path(&disk.path, 1),
            Self::partition_path(&disk.path, 2),
            SIMPLE_PARTITIONING_SUBVOLUMES
                .iter()
                .map(|(name, path)| format!(
                    "+-- {name} ({})",
                    if *path == "/" { "root" } else { path }
                ))
                .collect::<Vec<_>>()
                .join("\n")
        );

        if !ui::yesno("Confirm Destructive Operation", &confirm)? {
            return Ok(());
        }

        self.current_plan = Some(PartitionPlan::Simple {
            device: disk.path.clone(),
        });

        Ok(())
    }

    pub fn run_manual_partitioning(&mut self) -> Result<()> {
        ui::msgbox(
            "Manual Partitioning Mode",
            "You are entering manual partitioning shell.",
        )?;

        ui::clear();

        use ui::ansi::*;

        println!(
            "MANUAL PARTITIONING\n\n\
             Type 'exit' or press Ctrl+D to return to installer.\n\n\
             Please parition and format the drives manually.\n\
             Available tools: {YELLOW}fdisk{RESET}, {YELLOW}cfdisk{RESET}, {YELLOW}gdisk{RESET}, {YELLOW}parted{RESET}, {YELLOW}mkfs.*{RESET}, {YELLOW}btrfs{RESET}, etc.\n\
             Mount the devices under {YELLOW}/mnt{RESET} and return to the installer.\n\n"
        );

        let status = std::process::Command::new("bash")
            .arg("--norc")
            .env(
                "PS1",
                "[\\[\\e[36m\\]neoarch-installer manual partitioning\\[\\e[0m\\]]\\[\\n\\] - \\[\\e[33m\\]\\w \\[\\e[34m\\]# \\[\\e[0m\\]",
            )
            .status()?;

        if !status.success() {
            ui::msgbox(
                "Error",
                &format!("Shell exited with status: {}", status.code().unwrap_or(-1)),
            )?;
        }

        self.refresh_devices()?;

        let plan = detect_manual_partitioning()?;

        self.current_plan = Some(plan);

        Ok(())
    }
}

fn device_part(device: &str, part_n: u16) -> String {
    if device.contains("nvme") || device.contains("mmcblk") {
        format!("{device}p{part_n}")
    } else {
        format!("{device}{part_n}")
    }
}

impl PartitionPlan {
    pub fn commands(&self) -> Vec<(String, Vec<String>)> {
        let mut commands = Vec::new();

        if let PartitionPlan::Simple { device } = self {
            // remove GPT
            commands.push(cmdp!("wipefs", "-af", device.clone()));
            commands.push(cmdp!("sgdisk", "--zap-all", device.clone()));
            commands.push(cmdp!("sgdisk", "--clear", device.clone()));
            commands.push(cmdp!("partprobe", device.clone()));
            commands.push(cmdp!("udevadm", "settle"));

            // EFI
            commands.push(cmdp!(
                "parted",
                "-s",
                device.clone(),
                "mkpart",
                "ESP",
                "fat32",
                "1MiB",
                "1025MiB"
            ));

            commands.push(cmdp!(
                "parted",
                "-s",
                device.clone(),
                "set",
                "1",
                "esp",
                "on"
            ));

            // root
            commands.push(cmdp!(
                "parted",
                "-s",
                device.clone(),
                "mkpart",
                "primary",
                "btrfs",
                "1025MiB",
                "100%"
            ));

            // commit GPT
            commands.push(cmdp!("partprobe", device.clone()));
            commands.push(cmdp!("udevadm", "settle"));

            // mkfs
            commands.push(cmdp!(
                "mkfs.vfat",
                "-F32",
                "-n",
                "NEOARCH_EFI",
                device_part(device, 1)
            ));

            commands.push(cmdp!(
                "mkfs.btrfs",
                "-f",
                "-L",
                "NEOARCH_BTRFS_ROOT",
                device_part(device, 2)
            ));

            // mount root (temporary)
            commands.push(cmdp!("mount", device_part(device, 2), "/mnt"));

            // create subvolumes
            for (subvolume_name, _) in SIMPLE_PARTITIONING_SUBVOLUMES {
                commands.push(cmdp!(
                    "btrfs",
                    "subvolume",
                    "create",
                    format!("/mnt/{}", subvolume_name)
                ));
            }

            // unmount temporary root
            commands.push(cmdp!("umount", "/mnt"));

            // mount subvolumes
            for (subvolume_name, subvolume_path) in SIMPLE_PARTITIONING_SUBVOLUMES {
                commands.push(cmdp!(
                    "mount",
                    "-o",
                    format!("subvol={},compress=zstd,noatime", subvolume_name),
                    device_part(device, 2),
                    format!("/mnt{}", subvolume_path)
                        .trim_end_matches('/')
                        .to_string(),
                    "--mkdir"
                ));
            }

            // mount EFI
            commands.push(cmdp!(
                "mount",
                device_part(device, 1),
                "/mnt/boot/efi",
                "--mkdir"
            ));
        }

        commands
    }

    pub fn dry_run_commands(&self) -> String {
        self.commands()
            .iter()
            .map(|(cmd, args)| format!("{cmd} {}", args.join(" ")))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

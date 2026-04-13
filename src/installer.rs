use serde::Deserialize;

use crate::{
    cmdp,
    error::{Error, Result},
    partitioner::PartitionPlan,
    ui,
};

use std::{
    io::{BufRead, BufReader, Write},
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    thread,
};

#[derive(Debug, Deserialize)]
struct InstalledUser {
    username: String,
    password: Option<String>,
    admin: bool,
}

#[derive(Debug, Deserialize)]
struct InstallPlan {
    hostname: String,
    language: String,
    timezone: String,
    kernels: Vec<String>,
    init_system: String,
    system_packages: Vec<String>,
    users: Vec<InstalledUser>,
}

pub struct Installer {
    partition_plan: PartitionPlan,
}

impl Installer {
    pub fn new(partition_plan: PartitionPlan) -> Self {
        Self { partition_plan }
    }

    pub fn install(&self) -> Result<()> {
        if !ui::yesno("Proceed?", "Do you want to proceed with the installation")? {
            return Err(Error::Cancelled);
        }

        let plan_json = std::fs::read_to_string("/etc/neoarch-installer.json")?;
        let InstallPlan {
            hostname,
            language,
            timezone,
            kernels,
            init_system,
            system_packages,
            users,
        } = serde_json::from_str(&plan_json)?;

        let mut commands = self.partition_plan.commands();

        match init_system.as_str() {
            "openrc" => {
                commands.push(cmdp!("rc-service", "ntpd", "start"));
                commands.push(cmdp!("rc-update", "add", "ntpd"));
            }
            other => todo!("enabling and starting ntp on init system {other}"),
        }

        {
            let mut strap_cmd = cmdp!(
                if &init_system == "systemd" {
                    "pacstrap"
                } else {
                    "basestrap"
                },
                "/mnt",
                "base",
                "base-devel",
                "linux-firmware",
                "grub",
                "efibootmgr",
                "networkmanager",
                "napm",
                "neoarch-keyring",
                "ntp"
            );

            for kernel in &kernels {
                strap_cmd
                    .1
                    .extend_from_slice(&[kernel.clone(), format!("{kernel}-headers")]);
            }

            if &init_system != "systemd" {
                strap_cmd.1.extend_from_slice(&[
                    init_system.clone(),
                    "artix-archlinux-support".to_string(),
                    format!("elogind-{}", &init_system),
                    format!("networkmanager-{}", &init_system),
                    format!("ntp-{}", &init_system),
                ]);
            }

            strap_cmd.1.extend_from_slice(&system_packages);

            commands.push(strap_cmd);
        }

        if &init_system != "systemd" {
            commands.push(cmdp!(
                "cp",
                "-f",
                "/etc/pacman.d/mirrorlist-arch",
                "/mnt/etc/pacman.d/mirrorlist-arch"
            ));
        }

        commands.push(cmdp!(
            "cp",
            "-f",
            "/etc/pacman.conf",
            "/mnt/etc/pacman.conf"
        ));

        let chroot = if init_system == "systemd" {
            "arch-chroot"
        } else {
            "artix-chroot"
        };

        commands.push(cmdp!(chroot, "/mnt", "pacman-key", "--init"));

        commands.push(cmdp!(
            chroot,
            "/mnt",
            "pacman-key",
            "--populate",
            "artix",
            "archlinux",
            "neoarch"
        ));

        let fstab_gen = if init_system == "systemd" {
            "genfstab"
        } else {
            "fstabgen"
        };

        commands.push(cmdp!(
            "bash",
            "-c",
            format!("{fstab_gen} -U /mnt >> /mnt/etc/fstab")
        ));

        commands.push(cmdp!(
            chroot,
            "/mnt",
            "ln",
            "-sf",
            format!("/usr/share/zoneinfo/{timezone}"),
            "/etc/localtime"
        ));

        commands.push(cmdp!(chroot, "/mnt", "hwclock", "--systohc"));

        commands.push(cmdp!(
            chroot,
            "/mnt",
            "sed",
            "-i",
            format!("s/#{language}/{language}/g"),
            "/etc/locale.gen"
        ));

        commands.push(cmdp!(chroot, "/mnt", "locale-gen"));

        commands.push(cmdp!(
            chroot,
            "/mnt",
            "bash",
            "-c",
            format!("echo 'export LANG=\"{language}\"' > /etc/locale.conf"),
        ));

        commands.push(cmdp!(
            chroot,
            "/mnt",
            "bash",
            "-c",
            "echo 'export LC_COLLATE=\"C\"' >> /etc/locale.conf",
        ));

        commands.push(cmdp!(
            chroot,
            "/mnt",
            "sed",
            "-i",
            "s|^GRUB_DISTRIBUTOR=.*|GRUB_DISTRIBUTOR=\"NeoArch\"|",
            "/etc/default/grub"
        ));

        commands.push(cmdp!(
            chroot,
            "/mnt",
            "grub-install",
            "--target=x86_64-efi",
            "--efi-directory=/boot/efi",
            "--bootloader-id=neoarch-grub"
        ));

        commands.push(cmdp!(
            chroot,
            "/mnt",
            "grub-mkconfig",
            "-o",
            "/boot/grub/grub.cfg"
        ));

        commands.push(cmdp!(
            chroot,
            "/mnt",
            "bash",
            "-c",
            format!("echo '{hostname}' > /etc/hostname")
        ));

        if &init_system == "openrc" {
            commands.push(cmdp!(
                chroot,
                "/mnt",
                "bash",
                "-c",
                format!("echo \"hostname='{hostname}'\" > /etc/conf.d/hostname"),
            ));
        }

        commands.push(cmdp!(
            chroot,
            "/mnt",
            "bash",
            "-c",
            format!("echo '127.0.1.1 {hostname}.localdomain {hostname}' >> /etc/hosts"),
        ));

        match init_system.as_str() {
            "openrc" => {
                commands.push(cmdp!(chroot, "/mnt", "rc-update", "add", "NetworkManager"));
            }
            other => todo!("enabling NetworkManager on init system {other}"),
        }

        let mut passwordless_users = Vec::new();

        for InstalledUser {
            username,
            password,
            admin,
        } in &users
        {
            if username == "root" {
                if let Some(password_hash) = password {
                    commands.push(cmdp!(
                        chroot,
                        "/mnt",
                        "usermod",
                        username,
                        "-p",
                        password_hash
                    ));
                } else {
                    passwordless_users.push(username.clone());
                }
            } else {
                commands.push(cmdp!(chroot, "/mnt", "useradd", "-m", username));

                if let Some(password_hash) = password {
                    commands.push(cmdp!(
                        chroot,
                        "/mnt",
                        "usermod",
                        username,
                        "-p",
                        password_hash
                    ));
                } else {
                    passwordless_users.push(username.clone());
                }

                if *admin {
                    commands.push(cmdp!(
                        chroot, "/mnt", "usermod", username, "-a", "-G", "wheel"
                    ));
                }
            }
        }

        commands.push(cmdp!(
            chroot,
            "/mnt",
            "sed",
            "-i",
            "s|^# %wheel ALL=(ALL:ALL) ALL|%wheel ALL=(ALL:ALL) ALL|",
            "/etc/sudoers"
        ));

        let mut dialog = ui::programbox_start("Installing", "Running installation...")?;
        let stdin = dialog.stdin.take().unwrap();
        let stdin = Arc::new(Mutex::new(stdin));

        for cmd in commands {
            let cmd_display = format!("$ {} {}", cmd.0, cmd.1.join(" "));

            {
                let stdin_out = Arc::clone(&stdin);
                let mut stdin = stdin_out.lock().unwrap();
                writeln!(stdin, "{}", cmd_display)?;
            }

            let mut child = Command::new(cmd.0)
                .args(cmd.1)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()?;

            // stdout thread
            let stdin_out = Arc::clone(&stdin);
            let stdout = child.stdout.take().unwrap();
            let t1 = thread::spawn(move || -> Result<()> {
                let reader = BufReader::new(stdout);
                for line in reader.lines() {
                    let mut stdin = stdin_out.lock().unwrap();
                    writeln!(stdin, "{}", line?)?;
                }
                Ok(())
            });

            // stderr thread
            let stdin_err = Arc::clone(&stdin);
            let stderr = child.stderr.take().unwrap();
            let t2 = thread::spawn(move || -> Result<()> {
                let reader = BufReader::new(stderr);
                for line in reader.lines() {
                    let mut stdin = stdin_err.lock().unwrap();
                    writeln!(stdin, "{}", line?)?;
                }
                Ok(())
            });

            t1.join().unwrap()?;
            t2.join().unwrap()?;

            if !child.wait()?.success() {
                return Err(Error::InstallError(cmd_display));
            }
        }

        std::fs::write(
            "/mnt/etc/os-release",
            concat!(
                "NAME=\"NeoArch Linux\"\n",
                "PRETTY_NAME=\"NeoArch Linux\"\n",
                "ID=neoarch\n",
                "BUILD_ID=rolling\n",
                "ANSI_COLOR=\"38;2;67;185;238\"\n",
                "HOME_URL=\"https://neoarchlinux.org/\"\n",
                "DOCUMENTATION_URL=\"https://docs.neoarchlinux.org/\"\n",
                "LOGO=neoarchlinux-logo\n",
            ),
        )?;

        if !passwordless_users.is_empty() {
            ui::msgbox(
                "User password specification",
                "\
                        You have selected some users whose password you have not provided in the ISO.\n\
                        You will be prompted to provide them now.\
                ",
            )?;

            for username in passwordless_users {
                let password = loop {
                    let password = ui::passwordbox(
                        &format!("Password for {username}"),
                        &format!("Enter the password for user: {username}"),
                    )?;

                    let password1 = ui::passwordbox(
                        &format!("Password for {username}"),
                        &format!("Repeat the password for user: {username}"),
                    )?;

                    if password == password1 {
                        break password;
                    }

                    ui::msgbox(
                        "Password mismatch",
                        "Passwords do not match, please try again.",
                    )?;
                };

                let mut child = Command::new(chroot)
                    .args(["/mnt", "chpasswd"])
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()?;

                child
                    .stdin
                    .take()
                    .unwrap()
                    .write_all(format!("{username}:{password}").as_bytes())?;

                if !child.wait()?.success() {
                    return Err(Error::InstallError(format!(
                        "Failed to set password for {username}"
                    )));
                }
            }
        }

        ui::msgbox("Installation done", "Installation finished")?;

        Ok(())
    }
}

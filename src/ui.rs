use crate::error::{Error, Result};

use std::io::Write;
use std::process::{Command, Stdio};

pub mod ansi {
    pub const RESET: &str = "\x1b[0m";

    pub const BOLD: &str = "\x1b[1m";
    pub const DIM: &str = "\x1b[2m";
    pub const UNDERLINE: &str = "\x1b[4m";
    pub const BLINK: &str = "\x1b[5m";
    pub const REVERSE: &str = "\x1b[7m";
    pub const HIDDEN: &str = "\x1b[8m";

    pub const BLACK: &str = "\x1b[30m";
    pub const RED: &str = "\x1b[31m";
    pub const GREEN: &str = "\x1b[32m";
    pub const YELLOW: &str = "\x1b[33m";
    pub const BLUE: &str = "\x1b[34m";
    pub const MAGENTA: &str = "\x1b[35m";
    pub const CYAN: &str = "\x1b[36m";
    pub const WHITE: &str = "\x1b[37m";
}

pub fn clear() {
    print!("\x1b[2J\x1b[H");
}

const DIALOG: &str = "dialog";

const H: u32 = 40;
const W: u32 = 120;

fn run(extra_args: &[&str]) -> Result<(i32, String)> {
    let mut cmd = Command::new(DIALOG);
    cmd.arg("--keep-tite")
        .args(extra_args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::piped());

    let output = cmd.output()?;

    let code = output.status.code().unwrap_or(-1);
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    Ok((code, stderr))
}

fn run_selection(extra_args: &[&str]) -> Result<String> {
    let (code, stderr) = run(extra_args)?;

    match code {
        0 => Ok(stderr.trim().to_owned()),
        1 | 255 => Err(Error::Cancelled),
        _ => Err(Error::Dialog(format!("dialog exited with code {code}"))),
    }
}

pub fn msgbox(title: &str, text: &str) -> Result<()> {
    run(&[
        "--title",
        title,
        "--clear",
        "--msgbox",
        text,
        &H.to_string(),
        &W.to_string(),
    ])?;

    Ok(())
}

pub fn yesno(title: &str, text: &str) -> Result<bool> {
    let (code, _) = run(&[
        "--title",
        title,
        "--clear",
        "--yesno",
        text,
        &H.to_string(),
        &W.to_string(),
    ])?;

    Ok(code == 0)
}

pub fn menu(title: &str, prompt: &str, items: &[(impl ToString, impl ToString)]) -> Result<String> {
    let height = (items.len() + 15).min(30) as u32;
    let list_h = items.len().min(15) as u32;

    let height_s = height.to_string();
    let width_s = W.to_string();
    let listh_s = list_h.to_string();

    let mut args = vec![
        "--title", title, "--clear", "--menu", prompt, &height_s, &width_s, &listh_s,
    ];

    let flat: Vec<String> = items
        .iter()
        .flat_map(|(tag, desc)| [tag.to_string(), desc.to_string()])
        .collect();
    let flat_refs: Vec<&str> = flat.iter().map(|s| s.as_str()).collect();
    args.extend_from_slice(&flat_refs);

    run_selection(&args)
}

pub fn inputbox(title: &str, prompt: &str, init: &str) -> Result<String> {
    run_selection(&[
        "--title",
        title,
        "--clear",
        "--inputbox",
        prompt,
        &H.to_string(),
        &W.to_string(),
        init,
    ])
}

pub fn passwordbox(title: &str, prompt: &str) -> Result<String> {
    run_selection(&[
        "--title",
        title,
        "--clear",
        "--insecure",
        "--passwordbox",
        prompt,
        "10",
        &W.to_string(),
    ])
}

pub fn programbox_start(title: &str, text: &str) -> Result<std::process::Child> {
    let child = Command::new(DIALOG)
        .args([
            "--title",
            title,
            "--clear",
            "--programbox",
            text,
            "20",
            &W.to_string(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()?;

    Ok(child)
}

pub fn programbox_update(child: &mut std::process::Child, line: &str) -> Result<()> {
    if let Some(stdin) = child.stdin.as_mut() {
        writeln!(stdin, "{}", line)?;
    }
    Ok(())
}

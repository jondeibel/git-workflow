use std::io::Write;
use std::process::Command;

use anyhow::{Context, Result, bail};

pub fn run(shell: &str) -> Result<()> {
    if !matches!(shell, "zsh" | "bash" | "fish") {
        bail!("Unsupported shell: {shell}. Use zsh, bash, or fish.");
    }
    let executable = std::env::current_exe().context("could not locate the gw executable")?;
    let output = Command::new(executable)
        .env("COMPLETE", shell)
        .output()
        .context("failed to generate shell completions")?;
    if !output.status.success() {
        bail!(
            "failed to generate {shell} completions: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    std::io::stdout().write_all(&output.stdout)?;
    Ok(())
}

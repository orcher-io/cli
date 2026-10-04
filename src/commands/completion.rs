//! Implementation of the 'completion' command for generating shell completions
//! from the real command tree.

use crate::error::{CliError, Result};
use crate::utils::GlobalConfig;
use clap::CommandFactory;
use clap_complete::{generate, Shell};
use std::io;

/// Generate shell completion scripts
pub async fn execute(shell: Option<String>, global_config: &GlobalConfig) -> Result<()> {
    let shell = if let Some(shell_name) = shell {
        parse_shell(&shell_name)?
    } else {
        detect_shell()?
    };

    if !global_config.quiet {
        eprintln!("Generating completion script for {}...", shell_name(&shell));
    }

    let mut cmd = crate::cli::Cli::command();
    let bin_name = "orcher";

    generate(shell, &mut cmd, bin_name, &mut io::stdout());

    if !global_config.quiet {
        eprintln!("\n# Installation instructions:");
        print_installation_instructions(&shell);
    }

    Ok(())
}

/// Parse shell name string into Shell enum
fn parse_shell(shell_name: &str) -> Result<Shell> {
    match shell_name.to_lowercase().as_str() {
        "bash" => Ok(Shell::Bash),
        "zsh" => Ok(Shell::Zsh),
        "fish" => Ok(Shell::Fish),
        "powershell" | "pwsh" => Ok(Shell::PowerShell),
        "elvish" => Ok(Shell::Elvish),
        _ => Err(CliError::invalid_input(format!(
            "Unsupported shell: {}. Supported shells: bash, zsh, fish, powershell, elvish",
            shell_name
        ))),
    }
}

/// Detect current shell from environment
fn detect_shell() -> Result<Shell> {
    if let Ok(shell_path) = std::env::var("SHELL") {
        if shell_path.contains("bash") {
            return Ok(Shell::Bash);
        } else if shell_path.contains("zsh") {
            return Ok(Shell::Zsh);
        } else if shell_path.contains("fish") {
            return Ok(Shell::Fish);
        }
    }

    // Check for PowerShell on Windows
    if cfg!(windows) && std::env::var("PSModulePath").is_ok() {
        return Ok(Shell::PowerShell);
    }

    // Default to bash if can't detect
    Ok(Shell::Bash)
}

/// Get shell name as string
fn shell_name(shell: &Shell) -> &'static str {
    match shell {
        Shell::Bash => "bash",
        Shell::Zsh => "zsh",
        Shell::Fish => "fish",
        Shell::PowerShell => "powershell",
        Shell::Elvish => "elvish",
        _ => "unknown",
    }
}

/// Print installation instructions for each shell
fn print_installation_instructions(shell: &Shell) {
    match shell {
        Shell::Bash => {
            eprintln!("# For bash:");
            eprintln!("# Save the completion script to a file and source it in your .bashrc:");
            eprintln!("# orcher completion bash > ~/.orcher-completion.bash");
            eprintln!("# echo 'source ~/.orcher-completion.bash' >> ~/.bashrc");
            eprintln!("# source ~/.bashrc");
            eprintln!();
            eprintln!("# Or install globally:");
            eprintln!("# sudo orcher completion bash > /etc/bash_completion.d/orcher");
        }
        Shell::Zsh => {
            eprintln!("# For zsh:");
            eprintln!("# Save the completion script to a directory in your $fpath:");
            eprintln!("# mkdir -p ~/.zsh/completions");
            eprintln!("# orcher completion zsh > ~/.zsh/completions/_orcher");
            eprintln!("# Add to your .zshrc:");
            eprintln!("# fpath=(~/.zsh/completions $fpath)");
            eprintln!("# autoload -U compinit && compinit");
        }
        Shell::Fish => {
            eprintln!("# For fish:");
            eprintln!("# Save the completion script to fish's completions directory:");
            eprintln!("# orcher completion fish > ~/.config/fish/completions/orcher.fish");
            eprintln!("# Reload fish or restart your terminal");
        }
        Shell::PowerShell => {
            eprintln!("# For PowerShell:");
            eprintln!("# Add to your PowerShell profile:");
            eprintln!("# orcher completion powershell | Out-String | Invoke-Expression");
            eprintln!("# Or save to a file and dot-source it:");
            eprintln!("# orcher completion powershell > orcher-completion.ps1");
            eprintln!("# Add '. ./orcher-completion.ps1' to your profile");
        }
        Shell::Elvish => {
            eprintln!("# For elvish:");
            eprintln!("# Save the completion script and source it in your rc.elv:");
            eprintln!("# orcher completion elvish > orcher-completion.elv");
            eprintln!("# echo 'use ./orcher-completion' >> ~/.elvish/rc.elv");
        }
        _ => {
            eprintln!("# Installation instructions not available for this shell");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The completions cover the commands that exist, and only those: an
    /// earlier version completed a made-up tree (`apply`, `debug`, ...).
    #[test]
    fn completions_follow_the_real_command_tree() {
        let mut cmd = crate::cli::Cli::command();
        let mut out = Vec::new();
        generate(Shell::Bash, &mut cmd, "orcher", &mut out);
        let script = String::from_utf8(out).unwrap();
        for command in [
            "workflow",
            "namespace",
            "server",
            "dev",
            "auth",
            "batch",
            "queue",
        ] {
            assert!(script.contains(command), "missing {command}");
        }
        assert!(!script.contains("profile"), "a command that does not exist");
    }

    #[test]
    fn test_parse_shell() {
        assert!(matches!(parse_shell("bash"), Ok(Shell::Bash)));
        assert!(matches!(parse_shell("zsh"), Ok(Shell::Zsh)));
        assert!(matches!(parse_shell("fish"), Ok(Shell::Fish)));
        assert!(matches!(parse_shell("powershell"), Ok(Shell::PowerShell)));
        assert!(matches!(parse_shell("pwsh"), Ok(Shell::PowerShell)));
        assert!(matches!(parse_shell("elvish"), Ok(Shell::Elvish)));
        assert!(parse_shell("unknown").is_err());
    }

    #[test]
    fn test_shell_name() {
        assert_eq!(shell_name(&Shell::Bash), "bash");
        assert_eq!(shell_name(&Shell::Zsh), "zsh");
        assert_eq!(shell_name(&Shell::Fish), "fish");
        assert_eq!(shell_name(&Shell::PowerShell), "powershell");
        assert_eq!(shell_name(&Shell::Elvish), "elvish");
    }

    #[test]
    fn test_detect_shell() {
        // This test depends on environment, so just ensure it returns a valid shell
        let shell = detect_shell().unwrap();
        match shell {
            Shell::Bash | Shell::Zsh | Shell::Fish | Shell::PowerShell | Shell::Elvish => {}
            _ => panic!("Invalid shell detected"),
        }
    }
}

//! `orcher completion`: shell completion scripts.

use clap::{Args, CommandFactory};
use clap_complete::Shell;

#[derive(Args)]
#[command(after_help = "\
Install:
  bash        orcher completion bash > ~/.local/share/bash-completion/completions/orcher
  zsh         orcher completion zsh > \"${fpath[1]}/_orcher\"
  fish        orcher completion fish > ~/.config/fish/completions/orcher.fish
  powershell  orcher completion powershell >> $PROFILE")]
pub struct CompletionCommand {
    /// The shell to write completions for
    #[arg(value_enum)]
    shell: Shell,
}

pub fn run(cmd: CompletionCommand) {
    let mut command = crate::Cli::command();
    clap_complete::generate(cmd.shell, &mut command, "orcher", &mut std::io::stdout());
}

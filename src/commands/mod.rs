pub mod completion;
pub mod dev;
pub mod logs;
pub mod namespace;
pub mod workflow;

use crate::error::{Error, Result};
use std::io::IsTerminal;

/// Asks a yes/no question, unless `yes` already answered it. Without a
/// terminal to ask on, the answer must be given with `--yes`.
pub fn confirm(question: &str, yes: bool) -> Result<bool> {
    if yes {
        return Ok(true);
    }
    if !std::io::stdin().is_terminal() || !std::io::stderr().is_terminal() {
        return Err(Error::invalid_input(format!(
            "{question} Pass --yes to confirm when not running interactively."
        )));
    }
    dialoguer::Confirm::new()
        .with_prompt(question)
        .default(false)
        .interact()
        .map_err(|e| Error::local(format!("cannot read the answer: {e}")))
}

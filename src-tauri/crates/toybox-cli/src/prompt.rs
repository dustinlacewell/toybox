//! Asking before a destructive change, and reading secrets (never from arguments).

use std::io::{BufRead, Read};

use crate::ctx::Ctx;
use crate::error::{CliError, CliResult};
use crate::out;

/// Ask `question` before something that destroys data. Proceeds with `--yes`; asks on
/// a terminal; refuses without one.
pub fn confirm(ctx: &Ctx, question: &str) -> CliResult<()> {
    if ctx.g.yes {
        return Ok(());
    }
    if !out::stdin_is_terminal() {
        return Err(CliError::failed(format!("{question}: refusing to continue without a terminal to ask on")).hint("pass --yes to go ahead, or --dry-run to see what would change"));
    }
    out::stderr(&format!("{question} [y/N] "));
    let mut answer = String::new();
    std::io::stdin().lock().read_line(&mut answer)?;
    match answer.trim().to_lowercase().as_str() {
        "y" | "yes" => Ok(()),
        _ => Err(CliError::cancelled("cancelled, nothing was changed")),
    }
}

/// Everything on standard input.
pub fn read_stdin() -> CliResult<String> {
    let mut text = String::new();
    std::io::stdin().lock().read_to_string(&mut text)?;
    Ok(text)
}

/// An access token: standard input with `--token-stdin`, else `TOYBOX_TOKEN`, else a prompt
/// that does not echo.
pub fn read_token(token_stdin: bool) -> CliResult<String> {
    let token = if token_stdin {
        let mut line = String::new();
        std::io::stdin().lock().read_line(&mut line)?;
        line
    } else if let Some(token) = std::env::var("TOYBOX_TOKEN").ok().filter(|token| !token.trim().is_empty()) {
        token
    } else if out::stdin_is_terminal() {
        out::stderr("Access token: ");
        rpassword::read_password().map_err(|error| CliError::failed(format!("could not read the token: {error}")))?
    } else {
        return Err(CliError::usage("no access token given").hint("set TOYBOX_TOKEN, pass --token-stdin, or run in a terminal to be asked"));
    };
    let token = token.trim().to_string();
    if token.is_empty() {
        return Err(CliError::usage("the access token is empty"));
    }
    Ok(token)
}

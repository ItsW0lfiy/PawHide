//! CLI-first output. ANSI colour only in a real terminal, respecting NO_COLOR.
use std::io::{self, IsTerminal};
fn paint(code: &str, label: &str) -> String {
    let colour = io::stdout().is_terminal()
        && std::env::var_os("NO_COLOR").is_none()
        && std::env::var("CLICOLOR").ok().as_deref() != Some("0");
    if colour { format!("\x1b[{code}m{label}\x1b[0m") } else { label.to_owned() }
}
pub fn good(label: &str) -> String { paint("32",label) }
pub fn bad(label: &str) -> String { paint("31",label) }
pub fn warn(label: &str) -> String { paint("33",label) }
pub fn accent(label: &str) -> String { paint("36",label) }
pub fn dim(label: &str) -> String { paint("90",label) }
pub fn brief(text:&str, chars:usize) -> String {
    if text.chars().count() <= chars { return text.to_owned() }
    format!("{}…", text.chars().take(chars.saturating_sub(1)).collect::<String>())
}

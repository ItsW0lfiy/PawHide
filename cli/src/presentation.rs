//! PawHide's unified terminal presentation. Never color redirected output or JSON.
use std::io::{self, IsTerminal};

fn enabled() -> bool {
    io::stdout().is_terminal()
        && std::env::var_os("NO_COLOR").is_none()
        && std::env::var("CLICOLOR").ok().as_deref() != Some("0")
}
fn paint(code: &str, label: &str) -> String {
    if enabled() { format!("\x1b[{code}m{label}\x1b[0m") } else { label.to_owned() }
}
pub fn good(label: &str) -> String { paint("32", label) }
pub fn bad(label: &str) -> String { paint("31", label) }
pub fn warn(label: &str) -> String { paint("33", label) }
pub fn accent(label: &str) -> String { paint("95", label) }
pub fn dim(label: &str) -> String { paint("90", label) }

pub fn header(title: &str, explanation: &str) {
    println!();
    println!("{}", accent(&format!("PAWHIDE  /  {}", title)));
    println!("{}", dim(&"-".repeat(64.min(terminal_columns()))));
    if !explanation.is_empty() { println!("{}", dim(explanation)); }
    println!();
}
pub fn section(title: &str) {
    println!();
    println!("{}", accent(&format!("  {}", title)));
}
pub fn field(name: &str, value: impl std::fmt::Display) {
    println!("  {:<19} {}", name, value);
}
pub fn note(message: &str) {
    println!("{}", dim(&format!("  Note: {message}")));
}
pub fn next(message: &str) {
    println!("{}", dim(&format!("\n  Next: {message}")));
}
pub fn success(message: &str) {
    println!("  {}  {}", good("[OK]"), message);
}
pub fn warning(message: &str) {
    println!("  {}  {}", warn("[!]"), message);
}
pub fn error(message: &str) {
    eprintln!("  {}  {}", bad("[X]"), message);
}
pub fn brief(text: &str, chars: usize) -> String {
    if text.chars().count() <= chars { return text.to_owned(); }
    let keep = chars.saturating_sub(3);
    format!("{}...", text.chars().take(keep).collect::<String>())
}
fn terminal_columns() -> usize {
    std::env::var("COLUMNS").ok().and_then(|s| s.parse::<usize>().ok())
        .filter(|&n| (55..=300).contains(&n)).unwrap_or(110)
}
#[derive(Clone, Copy)]
pub enum Tone { Normal, Good, Warn, Bad, Dim }
pub struct Cell {
    pub text: String,
    pub tone: Tone,
}
impl Cell {
    pub fn plain(text: impl Into<String>) -> Self { Self { text: text.into(), tone: Tone::Normal } }
    pub fn good(text: impl Into<String>) -> Self { Self { text: text.into(), tone: Tone::Good } }
    pub fn warn(text: impl Into<String>) -> Self { Self { text: text.into(), tone: Tone::Warn } }
    pub fn bad(text: impl Into<String>) -> Self { Self { text: text.into(), tone: Tone::Bad } }
    pub fn dim(text: impl Into<String>) -> Self { Self { text: text.into(), tone: Tone::Dim } }
    fn render(&self, value: &str) -> String {
        match self.tone {
            Tone::Normal => value.to_owned(),
            Tone::Good => good(value),
            Tone::Warn => warn(value),
            Tone::Bad => bad(value),
            Tone::Dim => dim(value),
        }
    }
}
pub fn table(headers: &[&str], rows: &[Vec<Cell>]) {
    if headers.is_empty() { return; }
    let count = headers.len();
    let mut widths: Vec<usize> = headers.iter().map(|h| h.chars().count().max(7)).collect();
    for row in rows {
        for (i, c) in row.iter().enumerate().take(count) {
            widths[i] = widths[i].max(c.text.chars().count()).min(if i == 0 { 38 } else { 66 });
        }
    }
    let budget = terminal_columns().saturating_sub(4 + (count - 1) * 3);
    while widths.iter().sum::<usize>() > budget {
        if let Some((index, _)) = widths.iter().enumerate().filter(|(_, w)| **w > 10)
            .max_by_key(|(_, w)| **w)
        { widths[index] -= 1; } else { break; }
    }
    let line = headers.iter().enumerate().map(|(i, h)| format!("{:<width$}", brief(h, widths[i]), width = widths[i]))
        .collect::<Vec<_>>().join("   ");
    println!("  {}", accent(&line));
    println!("  {}", dim(&widths.iter().map(|w| "-".repeat(*w)).collect::<Vec<_>>().join("   ")));
    for row in rows {
        let rendered = (0..count).map(|i| {
            let value = row.get(i).map_or("", |c| c.text.as_str());
            let display_value = format!("{:<width$}", brief(value, widths[i]), width = widths[i]);
            row.get(i).map_or(display_value.clone(), |c| c.render(&display_value))
        }).collect::<Vec<_>>().join("   ");
        println!("  {rendered}");
    }
    if rows.is_empty() { println!("  {}", dim("(nothing to display)")); }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn truncates_by_char_count() {
        assert_eq!(brief("application.exe", 11), "applicat...");
        assert_eq!(brief("abc", 4), "abc");
    }
    #[test]
    fn colors_never_change_values() {
        assert_eq!(Cell::good("Protected").text, "Protected");
    }
}

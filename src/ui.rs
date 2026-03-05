use std::io::{self, BufRead, Write};

/// Ask a yes/no confirmation question. Returns true for yes.
/// In non-interactive mode (stdin is not a TTY), returns the default value.
pub fn confirm(prompt: &str, default: bool) -> bool {
    let suffix = if default { "[Y/n]" } else { "[y/N]" };

    // Check if stdin is a TTY
    if !atty_check() {
        return default;
    }

    print!("{prompt} {suffix} ");
    io::stdout().flush().ok();

    let mut input = String::new();
    if io::stdin().lock().read_line(&mut input).is_err() {
        return default;
    }

    let input = input.trim().to_lowercase();
    match input.as_str() {
        "y" | "yes" => true,
        "n" | "no" => false,
        "" => default,
        _ => default,
    }
}

/// Check if stdin is interactive.
fn atty_check() -> bool {
    use std::io::IsTerminal;
    std::io::stdin().is_terminal()
}

/// Print a success message.
pub fn success(msg: &str) {
    use colored::Colorize;
    println!("{}", msg.green());
}

/// Print a warning message to stderr.
pub fn warn(msg: &str) {
    use colored::Colorize;
    eprintln!("{}", msg.yellow());
}

/// Print an error message to stderr.
pub fn error(msg: &str) {
    use colored::Colorize;
    eprintln!("{}", msg.red());
}

/// Print an info message.
pub fn info(msg: &str) {
    println!("{msg}");
}

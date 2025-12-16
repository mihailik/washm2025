use crate::brush_core::error;
use crate::brush_core::parser::ast::SourceLocation;

/// Host shim for interpreter error logging. Prints up to five lines of context (no filename),
/// prefixed with line numbers. Context lines are dimmed, the error line is normal text.
pub fn log_interp_error<T: std::fmt::Debug + SourceLocation>(command: &T, err: &error::Error, shell: &crate::brush_core::Shell) {
    // Debug marker to confirm this host shim is invoked at runtime.
    eprintln!("HOST-SNIPPET: interp_log_shim invoked");
    // Try to extract SourceSpan from the AST node
    if let Some(span) = command.location() {
        let source_info = shell.call_stack().current_pos_as_source_info();

        // Try to read source text if source_info references a real file
        if let Ok(src_text) = std::fs::read_to_string(&source_info.source) {
            let lines: Vec<&str> = src_text.lines().collect();
            if !lines.is_empty() {
                let source_start_line = source_info.start.as_ref().map(|p| p.line).unwrap_or(1);
                let error_line = source_start_line.saturating_add(span.start.line.saturating_sub(1));
                let err_idx = error_line.saturating_sub(1) as usize;

                let total = lines.len();
                let half: isize = 2; // show two lines before and after (total up to 5)
                let first = ((err_idx as isize - half).max(0)) as usize;
                let last = ((err_idx as isize + half).min(total as isize - 1)) as usize;

                let ln_width = (last + 1).to_string().len();

                for i in first..=last {
                    let ln = i + 1;
                    let text = lines.get(i).copied().unwrap_or("");
                    if i == err_idx {
                        // error line: show '>' marker
                        eprintln!("{num:>width$} > {text}", num=ln, width=ln_width, text=text);
                    } else {
                        // context lines: dim gray and use spaces instead of pipe
                        eprintln!("\x1b[90m{num:>width$}   {text}\x1b[0m", num=ln, width=ln_width, text=text);
                    }
                }

                eprintln!("\nerror: {}", err);
                eprintln!();
                return;
            }
        }
    }

    // Fallback: produce compact hints from Debug output and show the error
    let cmd_debug = format!("{:?}", command);
    match err.kind() {
        error::ErrorKind::IoError(io_err) => {
            eprintln!("\nerror: i/o error: {}", io_err);
            // Extract some tokens and paths for hints
            let mut paths = Vec::new();
            let mut hints = Vec::new();
            for token in cmd_debug.split(|c: char| !c.is_ascii_graphic()) {
                if token.starts_with('/') || token.starts_with("~/") || token.contains("/etc/") {
                    paths.push(token.to_string());
                }
                if token == "-r" || token == "-x" || token == "-z" {
                    hints.push(token.to_string());
                }
                if token.contains("lesspipe") || token.contains("dircolors") || token.contains("cat") || token.contains("eval") {
                    hints.push(token.to_string());
                }
            }
            if !paths.is_empty() {
                eprintln!("  Referenced paths: {}", paths.join(", "));
            }
            if !hints.is_empty() {
                eprintln!("  Detected tokens: {}", hints.join(", "));
            }
            eprintln!("  OS error kind: {:?}", io_err.kind());
        }
        error::ErrorKind::NotSupportedOnThisPlatform(detail) => {
            eprintln!("\nerror: operation not supported on this platform: {}", detail);
            let mut paths = Vec::new();
            for token in cmd_debug.split(|c: char| !c.is_ascii_graphic()) {
                if token.starts_with('/') || token.starts_with("~/") || token.contains("/etc/") {
                    paths.push(token.to_string());
                }
            }
            if !paths.is_empty() {
                eprintln!("  Referenced paths: {}", paths.join(", "));
            }
        }
        _ => {
            eprintln!("\nerror: {}", err);
        }
    }

    eprintln!();
}

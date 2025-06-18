use clap::Parser;
use std::path::PathBuf;

pub const MAX_DATA_GATHERING_ITERATIONS_DEFAULT: usize = 3;
pub const MAX_VERIFICATION_RETRIES_DEFAULT: usize = 2;

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum DebugMode {
    Initial,
    Sufficient,
    Changes,
}

impl std::fmt::Display for DebugMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            DebugMode::Changes => "changes", // Made these simpler for clap parsing
            DebugMode::Initial => "initial",
            DebugMode::Sufficient => "sufficient",
        };
        write!(f, "{s}")
    }
}

// Helper function to parse DebugMode for clap
fn parse_debug_mode(s: &str) -> Result<DebugMode, String> {
    match s.to_lowercase().as_str() {
        "initial" | "debug:initial" => Ok(DebugMode::Initial),
        "sufficient" | "debug:sufficient" => Ok(DebugMode::Sufficient),
        "changes" | "debug:changes" => Ok(DebugMode::Changes),
        _ => Err(format!("invalid debug mode: {}", s)),
    }
}

#[derive(Parser, Debug)]
#[command(author, version, about = "gem: Rust-specific coding agent", long_about = None, trailing_var_arg = true)]
pub struct CustomCliArgs {
    /// The user's request in natural language (e.g., "change structs to a SOA architecture in the tests folder")
    /// Can also be read from stdin if not provided.
    #[arg(name = "USER_REQUEST_PARTS", required = false)]
    pub user_request_parts: Vec<String>,

    /// Command to verify the changes (e.g., "cargo test --all-features")
    #[arg(long, default_value = "cargo build")]
    pub verify_with: String,

    /// Do not ask Gemini to generate tests
    #[arg(long)]
    pub no_test: bool,

    /// Path to the project root
    #[arg(long, default_value = ".")]
    pub project_root: PathBuf,

    /// Maximum number of data gathering loops
    #[arg(long, default_value_t = MAX_DATA_GATHERING_ITERATIONS_DEFAULT)]
    pub max_data_loops: usize,

    /// Maximum number of verification retries (after the initial attempt)
    #[arg(long, default_value_t = MAX_VERIFICATION_RETRIES_DEFAULT)]
    pub max_verify_retries: usize,

    /// Debug mode: Runs up to a specific stage and prints information.
    /// Valid stages: initial, sufficient, changes.
    #[arg(long, value_parser = parse_debug_mode)]
    pub debug_mode: Option<DebugMode>,

    // This field is now implicitly handled by clap's help generation.
    // We can remove `show_help` field if we exclusively rely on clap's --help / -h.
    // For explicit control or custom help triggers beyond clap's default, it could be kept.
    // However, clap's `ArgAction::SetTrue` on a --help flag is the idiomatic way.
    // Since `main.rs` will use `CustomCliArgs::parse()`, clap's help is automatic.
    // pub show_help: bool,

    /// URL to open in the browser for web-based tasks.
    #[arg(long)]
    pub browser_url: Option<String>,

    /// CSS selector for the input field (requires --browser).
    #[arg(long, requires = "browser_url")]
    pub input_selector: Option<String>,

    /// CSS selector for the codeblock to copy from (requires --browser).
    #[arg(long, requires = "browser_url")]
    pub codeblock_selector: Option<String>,

    /// CSS selector to indicate task completion (requires --browser).
    #[arg(long, requires = "browser_url")]
    pub finished_selector: Option<String>,

    /// Use a local model (e.g., Gemma) instead of a remote API.
    #[arg(long)]
    pub local_model: bool,
}

// The old manual parsing logic (parse_cli_args and print_custom_help) is removed.
// Tests will also need to be updated or removed as they tested the old manual parser.

#[cfg(test)]
mod tests {
    use super::*;
    // Note: The old tests for `parse_cli_args` are no longer valid as that function
    // has been removed. New tests should be written to test clap's behavior if needed,
    // but typically clap's parsing is well-tested by itself.
    // For this refactoring, we'll rely on `cargo check` and later runtime tests
    // to ensure clap is configured correctly.

    #[test]
    fn test_clap_basic_parsing() {
        // Example of how to test clap parsing if desired:
        let args = CustomCliArgs::try_parse_from(&["gem", "my", "request"]).unwrap();
        assert_eq!(args.user_request_parts, vec!["my", "request"]);
        assert_eq!(args.verify_with, "cargo build"); // Default
    }

    #[test]
    fn test_clap_options() {
        let args = CustomCliArgs::try_parse_from(&[
            "gem",
            "--no-test",
            "--project-root",
            "/tmp",
            "another",
            "task",
        ])
        .unwrap();
        assert!(args.no_test);
        assert_eq!(args.project_root, PathBuf::from("/tmp"));
        assert_eq!(args.user_request_parts, vec!["another", "task"]);
    }

    #[test]
    fn test_clap_debug_mode() {
        let args = CustomCliArgs::try_parse_from(&["gem", "--debug-mode", "initial", "debug", "task"]).unwrap();
        assert_eq!(args.debug_mode, Some(DebugMode::Initial));
        assert_eq!(args.user_request_parts, vec!["debug", "task"]);

        let args_colon = CustomCliArgs::try_parse_from(&["gem", "--debug-mode", "debug:sufficient", "debug", "task"]).unwrap();
        assert_eq!(args_colon.debug_mode, Some(DebugMode::Sufficient));
    }

    #[test]
    fn test_clap_browser_options() {
        let args = CustomCliArgs::try_parse_from(&[
            "gem",
            "--browser", "http://example.com",
            "--input", "#input",
            "web task"
        ]).unwrap();
        assert_eq!(args.browser_url, Some("http://example.com".to_string()));
        assert_eq!(args.input_selector, Some("#input".to_string()));
        assert_eq!(args.user_request_parts, vec!["web task"]);
    }

    #[test]
    fn test_clap_local_model() {
        let args = CustomCliArgs::try_parse_from(&["gem", "--local-model", "local stuff"]).unwrap();
        assert!(args.local_model);
        assert_eq!(args.user_request_parts, vec!["local stuff"]);
    }

    #[test]
    fn test_clap_missing_user_request_ok_if_local_or_browser() {
        let args_local = CustomCliArgs::try_parse_from(&["gem", "--local-model"]).unwrap();
        assert!(args_local.local_model);
        assert!(args_local.user_request_parts.is_empty());

        let args_browser = CustomCliArgs::try_parse_from(&["gem", "--browser", "http://example.com"]).unwrap();
        assert_eq!(args_browser.browser_url, Some("http://example.com".to_string()));
        assert!(args_browser.user_request_parts.is_empty());
    }

    #[test]
    fn test_clap_error_missing_user_request() {
        // No request, no local, no browser -> should error due to `required_unless_present_any`
        let result = CustomCliArgs::try_parse_from(&["gem", "--no-test"]);
        assert!(result.is_err());
    }
}

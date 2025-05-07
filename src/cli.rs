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
            DebugMode::Changes => "debug:changes",
            DebugMode::Initial => "debug:initial",
            DebugMode::Sufficient => "debug:sufficient",
        };
        write!(f, "{s}")
    }
}

#[derive(Debug)]
pub struct CustomCliArgs {
    pub user_request: String,
    pub verify_with: String,
    pub no_test: bool,
    pub project_root: PathBuf,
    pub max_data_loops: usize,
    pub max_verify_retries: usize,
    pub debug_mode: Option<DebugMode>,
    pub show_help: bool,
}

impl Default for CustomCliArgs {
    fn default() -> Self {
        CustomCliArgs {
            user_request: String::new(),
            verify_with: "cargo build".to_string(),
            no_test: false,
            project_root: PathBuf::from("."),
            max_data_loops: MAX_DATA_GATHERING_ITERATIONS_DEFAULT,
            max_verify_retries: MAX_VERIFICATION_RETRIES_DEFAULT,
            debug_mode: None,
            show_help: false,
        }
    }
}

pub fn parse_cli_args(raw_args: Vec<String>) -> Result<CustomCliArgs, String> {
    let mut cli_args = CustomCliArgs::default();
    let mut args_iter = raw_args.iter().skip(1).peekable(); // Skip program name

    let mut request_parts: Vec<String> = Vec::new();
    let mut parsing_options_phase = true; // True while we are expecting options or the first request part

    // Handle empty arguments or "help" as the very first argument
    if args_iter.peek().is_none() {
        cli_args.show_help = true;
        return Ok(cli_args);
    }

    if let Some(first_arg_str) = args_iter.peek().map(|s| s.as_str()) {
        if first_arg_str == "help" || first_arg_str == "--help" {
            cli_args.show_help = true;
            // Consume the help argument
            args_iter.next();
            // If "help" is the first argument, other arguments might be present but are ignored as per test_parse_help_with_other_args
            return Ok(cli_args);
        }
    }

    // Main parsing loop
    while let Some(arg_raw) = args_iter.next() {
        let arg = arg_raw.clone(); // Clone to satisfy borrow checker for args_iter.next() in match arms

        if parsing_options_phase && arg.starts_with("--") {
            match arg.as_str() {
                "--verify-with" => {
                    cli_args.verify_with = args_iter
                        .next()
                        .cloned()
                        .ok_or_else(|| "Missing value for --verify-with".to_string())?;
                }
                "--no-test" => {
                    cli_args.no_test = true;
                }
                "--project-root" => {
                    cli_args.project_root = PathBuf::from(
                        args_iter
                            .next()
                            .cloned()
                            .ok_or_else(|| "Missing value for --project-root".to_string())?,
                    );
                }
                "--max-data-loops" => {
                    let val_str = args_iter
                        .next()
                        .cloned()
                        .ok_or_else(|| "Missing value for --max-data-loops".to_string())?;
                    cli_args.max_data_loops = val_str
                        .parse()
                        .map_err(|e| format!("Invalid value for --max-data-loops: {}", e))?;
                }
                "--max-verify-retries" => {
                    let val_str = args_iter
                        .next()
                        .cloned()
                        .ok_or_else(|| "Missing value for --max-verify-retries".to_string())?;
                    cli_args.max_verify_retries = val_str
                        .parse()
                        .map_err(|e| format!("Invalid value for --max-verify-retries: {}", e))?;
                }
                "--help" => {
                    cli_args.show_help = true;
                    return Ok(cli_args); // --help option also triggers help and exits
                }
                _ => {
                    // Unknown option starting with "--", treat as part of the request.
                    request_parts.push(arg);
                    parsing_options_phase = false; // Switch to request parsing mode
                }
            }
        } else {
            // This is not an option starting with "--", or we are already past the options phase.
            if parsing_options_phase {
                // This is the first non-option argument encountered.
                parsing_options_phase = false; // Subsequent args are definitely part of the request.

                // Check if this first request part is a debug mode specifier.
                // This check only happens if debug_mode hasn't been set by a (hypothetical future) --debug option.
                if cli_args.debug_mode.is_none() {
                    if arg == "debug:initial" {
                        cli_args.debug_mode = Some(DebugMode::Initial);
                        // Do not add "debug:initial" to request_parts
                    } else if arg == "debug:sufficient" {
                        cli_args.debug_mode = Some(DebugMode::Sufficient);
                        // Do not add "debug:sufficient" to request_parts
                    } else if arg == "debug:changes" {
                        cli_args.debug_mode = Some(DebugMode::Changes);
                        // Do not add "debug:changes" to request_parts
                    } else {
                        // Not a recognized debug mode string (e.g., "debug:", "debug:foo", or regular request word).
                        // It's part of the user request.
                        request_parts.push(arg);
                    }
                } else {
                    // Debug mode was already set (e.g., by a hypothetical future "--debug initial" option),
                    // so this 'arg' must be a request part.
                    request_parts.push(arg);
                }
            } else {
                // We are already in the request part collection phase.
                request_parts.push(arg);
            }
        }
    }

    cli_args.user_request = request_parts.join(" ");

    // If show_help is set (e.g. by "gem help" or "gem --help"), user_request can be empty.
    // Otherwise, if no user request is provided (and not a debug-only command without request), it's an error.
    if !cli_args.show_help && cli_args.user_request.is_empty() {
        // Check if it's a debug mode command that might not need a user_request
        if cli_args.debug_mode.is_none() {
            // If not in debug mode, request is mandatory
            return Err(
                "User request is missing. Use 'gem help' for usage information.".to_string(),
            );
        }
        // If in debug mode, an empty request might be permissible for some debug stages
        // (e.g., just to print a prompt without further context).
        // The current tests imply "gem debug:initial" without a request is fine.
    }

    Ok(cli_args)
}

pub fn print_custom_help() {
    println!(
        r#"gem: Rust-specific coding agent for Gemini Thinking

USAGE:
    gem [OPTIONS] <USER_REQUEST>
    gem help
    gem debug:<DEBUG_STAGE> [OPTIONS] <USER_REQUEST>

ARGS:
    <USER_REQUEST>
        The user's request in natural language (e.g., "change structs to a SOA architecture in the tests folder")
        If using a debug stage, this request follows the debug stage.

DEBUG STAGES (prefix to the request):
    debug:initial       Run up to the initial information gathering prompt and print it.
    debug:sufficient    Run up to the data sufficiency check prompt and print it.
    debug:changes       Run up to the code generation prompt (first attempt) and print it.

OPTIONS:
    --verify-with <COMMAND>
        Command to verify the changes (e.g., "cargo test --all-features")
        [default: cargo build]

    --no-test
        Do not ask Gemini to generate tests

    --project-root <PATH>
        Path to the project root
        [default: .]

    --max-data-loops <NUMBER>
        Maximum number of data gathering loops
        [default: {}]

    --max-verify-retries <NUMBER>
        Maximum number of verification retries (after the initial attempt)
        [default: {}]

    --help
        Print help information
"#,
        MAX_DATA_GATHERING_ITERATIONS_DEFAULT, MAX_VERIFICATION_RETRIES_DEFAULT
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn str_vec(args: &[&str]) -> Vec<String> {
        std::iter::once("gem_program_name".to_string())
            .chain(args.iter().map(|s| s.to_string()))
            .collect()
    }

    #[test]
    fn test_parse_simple_request() {
        let args = parse_cli_args(str_vec(&["create a new function"])).unwrap();
        assert_eq!(args.user_request, "create a new function");
        assert_eq!(args.verify_with, "cargo build"); // Default
        assert!(!args.no_test);
    }

    #[test]
    fn test_parse_with_options() {
        let args = parse_cli_args(str_vec(&[
            "refactor module X",
            "--verify-with",
            "cargo test",
            "--no-test",
            "--project-root",
            "/tmp/myproj",
            "--max-data-loops",
            "5",
            "--max-verify-retries",
            "3",
        ]))
        .unwrap();
        assert_eq!(args.user_request, "refactor module X");
        assert_eq!(args.verify_with, "cargo test");
        assert!(args.no_test);
        assert_eq!(args.project_root, PathBuf::from("/tmp/myproj"));
        assert_eq!(args.max_data_loops, 5);
        assert_eq!(args.max_verify_retries, 3);
    }

    #[test]
    fn test_parse_request_with_hyphens_in_it_before_options() {
        let args = parse_cli_args(str_vec(&[
            "fix",
            "issue",
            "--foo",
            "bar",
            "--verify-with",
            "cargo check",
        ]))
        .unwrap();
        assert_eq!(args.user_request, "fix issue --foo bar");
        assert_eq!(args.verify_with, "cargo check");
    }

    #[test]
    fn test_parse_options_first() {
        let args = parse_cli_args(str_vec(&[
            "--verify-with",
            "cargo check",
            "fix",
            "issue",
            "--foo",
            "bar",
        ]))
        .unwrap();
        assert_eq!(args.user_request, "fix issue --foo bar"); // Order of req parts matters
        assert_eq!(args.verify_with, "cargo check");
    }

    #[test]
    fn test_parse_help_long() {
        let args = parse_cli_args(str_vec(&["--help"])).unwrap();
        assert!(args.show_help);
    }

    #[test]
    fn test_parse_help_short() {
        let args = parse_cli_args(str_vec(&["help"])).unwrap();
        assert!(args.show_help);
    }

    #[test]
    fn test_parse_help_with_other_args() {
        // "help" as the first command argument should trigger help, regardless of subsequent tokens.
        let args = parse_cli_args(str_vec(&["help", "do", "something"])).unwrap();
        assert!(args.show_help);
        assert_eq!(args.user_request, ""); // Request should be empty if help is shown this way
    }

    #[test]
    fn test_parse_no_request() {
        // Test case 1: No arguments at all (just program name)
        let args_empty = parse_cli_args(str_vec(&[])).unwrap();
        assert!(args_empty.show_help); // Should show help

        // Test case 2: Only options, no request
        let err_opts_only = parse_cli_args(str_vec(&["--no-test"])).unwrap_err();
        assert!(err_opts_only.contains("User request is missing"));
    }

    #[test]
    fn test_missing_value_for_option() {
        let err = parse_cli_args(str_vec(&["request", "--verify-with"])).unwrap_err();
        assert_eq!(err, "Missing value for --verify-with");
    }

    #[test]
    fn test_invalid_value_for_option() {
        let err = parse_cli_args(str_vec(&["request", "--max-data-loops", "five"])).unwrap_err();
        assert!(err.contains("Invalid value for --max-data-loops"));
    }

    #[test]
    fn test_unknown_option_becomes_request() {
        // This test verifies that an unrecognized "--option" becomes part of the request
        let args = parse_cli_args(str_vec(&["request", "--unknown-flag", "value"])).unwrap();
        assert_eq!(args.user_request, "request --unknown-flag value");
    }

    #[test]
    fn test_debug_initial_request() {
        let args = parse_cli_args(str_vec(&["debug:initial", "my", "test", "request"])).unwrap();
        assert_eq!(args.debug_mode, Some(DebugMode::Initial));
        assert_eq!(args.user_request, "my test request");
    }

    #[test]
    fn test_debug_initial_request_alternative_spacing() {
        let args = parse_cli_args(str_vec(&["debug:initial", "my request"])).unwrap();
        assert_eq!(args.debug_mode, Some(DebugMode::Initial));
        assert_eq!(args.user_request, "my request");
    }

    #[test]
    fn test_debug_sufficient_request_with_options() {
        let args = parse_cli_args(str_vec(&[
            "debug:sufficient",
            "another",
            "task",
            "--no-test",
        ]))
        .unwrap();
        assert_eq!(args.debug_mode, Some(DebugMode::Sufficient));
        assert_eq!(args.user_request, "another task");
        assert!(args.no_test);
    }

    #[test]
    fn test_debug_changes_request_options_mixed() {
        let args = parse_cli_args(str_vec(&[
            "--project-root",
            "/dev/null",
            "debug:changes",
            "code gen task",
        ]))
        .unwrap();
        assert_eq!(args.debug_mode, Some(DebugMode::Changes));
        assert_eq!(args.user_request, "code gen task");
        assert_eq!(args.project_root, PathBuf::from("/dev/null"));
    }

    #[test]
    fn test_request_starting_with_debug_literal() {
        // If it's not a known debug mode, "debug:" is part of the request
        let args = parse_cli_args(str_vec(&["debug:", "my", "actual", "request"])).unwrap();
        assert_eq!(args.debug_mode, None);
        assert_eq!(args.user_request, "debug: my actual request");
    }

    #[test]
    fn test_request_starting_with_unrecognized_debug_prefix() {
        let args = parse_cli_args(str_vec(&["debug:foo", "my", "request"])).unwrap();
        assert_eq!(args.debug_mode, None);
        assert_eq!(args.user_request, "debug:foo my request");
    }

    #[test]
    fn test_debug_mode_no_request() {
        let args = parse_cli_args(str_vec(&["debug:initial"])).unwrap();
        assert_eq!(args.debug_mode, Some(DebugMode::Initial));
        assert_eq!(args.user_request, ""); // Empty request is fine with debug mode
        assert!(!args.show_help);
    }

    #[test]
    fn test_options_after_request_part_are_part_of_request() {
        let args =
            parse_cli_args(str_vec(&["do", "this", "--then-this-option-like-thing"])).unwrap();
        assert_eq!(args.user_request, "do this --then-this-option-like-thing");
    }
}

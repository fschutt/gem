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
    let mut args_iter = raw_args.clone().into_iter().skip(1); // Skip program name

    let mut request_parts: Vec<String> = Vec::new();
    let mut first_arg_processed = false;

    // Peek at the first argument for 'help' or debug modes that might not be options
    if let Some(first_real_arg) = args_iter.next() {
        if first_real_arg == "help" || first_real_arg == "--help" {
            cli_args.show_help = true;
            return Ok(cli_args);
        }

        // Check for debug prefixes on the very first part of the request
        if first_real_arg.starts_with("debug:") {
            if first_real_arg == "debug:" || first_real_arg == "debug:initial" {
                cli_args.debug_mode = Some(DebugMode::Initial);
                // The rest of the request will be collected later
            } else if first_real_arg == "debug:sufficient" {
                cli_args.debug_mode = Some(DebugMode::Sufficient);
            } else if first_real_arg == "debug:changes" {
                cli_args.debug_mode = Some(DebugMode::Changes);
            } else {
                // Not a special debug mode, treat "debug:..." as part of the request
                request_parts.push(first_real_arg);
            }
        } else {
            request_parts.push(first_real_arg);
        }
        first_arg_processed = true;
    } else {
        // No arguments provided after program name
        cli_args.show_help = true; // Show help if no args
        return Ok(cli_args);
    }


    // Process remaining arguments
    let mut current_args_iter = if first_arg_processed { args_iter } else { raw_args.into_iter().skip(1) };

    while let Some(arg) = current_args_iter.next() {
        if arg.starts_with("--") {
            match arg.as_str() {
                "--verify-with" => {
                    cli_args.verify_with = current_args_iter.next().ok_or_else(|| "Missing value for --verify-with".to_string())?;
                }
                "--no-test" => {
                    cli_args.no_test = true;
                }
                "--project-root" => {
                    cli_args.project_root = PathBuf::from(current_args_iter.next().ok_or_else(|| "Missing value for --project-root".to_string())?);
                }
                "--max-data-loops" => {
                    let val_str = current_args_iter.next().ok_or_else(|| "Missing value for --max-data-loops".to_string())?;
                    cli_args.max_data_loops = val_str.parse().map_err(|e| format!("Invalid value for --max-data-loops: {}", e))?;
                }
                "--max-verify-retries" => {
                    let val_str = current_args_iter.next().ok_or_else(|| "Missing value for --max-verify-retries".to_string())?;
                    cli_args.max_verify_retries = val_str.parse().map_err(|e| format!("Invalid value for --max-verify-retries: {}", e))?;
                }
                "--help" => {
                    cli_args.show_help = true;
                    return Ok(cli_args);
                }
                _ => return Err(format!("Unknown option: {}", arg)),
            }
        } else {
            // Argument is part of the user request
            request_parts.push(arg);
        }
    }

    cli_args.user_request = request_parts.join(" ");

    // If a debug mode was set by "debug:mode" as the first word, user_request will currently be empty.
    // If "debug:mode actual request" then user_request will be "actual request".
    // We need to ensure that if a debug mode is active, the "debug:mode" part is not in the user_request.

    if cli_args.debug_mode.is_some() {
        if cli_args.user_request.starts_with("debug:initial") {
             cli_args.user_request = cli_args.user_request.strip_prefix("debug:initial").unwrap_or("").trim_start().to_string();
        } else if cli_args.user_request.starts_with("debug:sufficient") {
             cli_args.user_request = cli_args.user_request.strip_prefix("debug:sufficient").unwrap_or("").trim_start().to_string();
        } else if cli_args.user_request.starts_with("debug:changes") {
             cli_args.user_request = cli_args.user_request.strip_prefix("debug:changes").unwrap_or("").trim_start().to_string();
        }
    }


    if !cli_args.show_help && cli_args.user_request.is_empty() {
        return Err("User request is missing. Use 'gem help' for usage information.".to_string());
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
        ])).unwrap();
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
        ])).unwrap();
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
        let args = parse_cli_args(str_vec(&["help", "do", "something"])).unwrap();
        assert!(args.show_help);
    }


    #[test]
    fn test_parse_no_request() {
        let err = parse_cli_args(str_vec(&[])).unwrap_err(); // No args after program name
        assert!(err.contains("User request is missing")); // Should actually trigger show_help path

        let args_help = parse_cli_args(str_vec(&[])).unwrap();
        assert!(args_help.show_help);


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
    fn test_unknown_option() {
        let err = parse_cli_args(str_vec(&["request", "--unknown-flag"])).unwrap_err();
        assert_eq!(err, "Unknown option: --unknown-flag");
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
            "--project-root", "/dev/null", "debug:changes", "code gen task"
        ])).unwrap();
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
}
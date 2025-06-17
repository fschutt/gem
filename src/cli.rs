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
    // Browser mode options
    pub browser_url: Option<String>,
    pub input_selector: Option<String>,
    pub codeblock_selector: Option<String>,
    pub finished_selector: Option<String>,
    pub local_model: bool,
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
            browser_url: None,
            input_selector: None,
            codeblock_selector: None,
            finished_selector: None,
            local_model: false,
        }
    }
}

pub fn parse_cli_args(raw_args: Vec<String>) -> Result<CustomCliArgs, String> {
    let mut cli_args = CustomCliArgs::default();
    let args: Vec<String> = raw_args.into_iter().skip(1).collect(); // Skip program name

    if args.is_empty() {
        cli_args.show_help = true;
        return Ok(cli_args);
    }

    if args.len() == 1 && (args[0] == "help" || args[0] == "--help") {
        cli_args.show_help = true;
        return Ok(cli_args);
    }

    // If "help" or "--help" is present anywhere, show help and ignore other args/options.
    // This simplifies things and matches common CLI behavior.
    if args.iter().any(|arg| arg == "help" || arg == "--help") {
        cli_args.show_help = true;
        return Ok(cli_args);
    }

    let mut request_parts: Vec<String> = Vec::new();
    let mut consumed_indices: Vec<usize> = Vec::new(); // To keep track of indices of consumed args
    let mut browser_flag_found = false; // Tracks if --browser flag itself was seen

    // Pass 1: Parse options and debug modes
    let mut i = 0;
    while i < args.len() {
        if consumed_indices.contains(&i) {
            i += 1;
            continue;
        }

        let arg = &args[i];
        let mut consumed_current_arg = false;

        match arg.as_str() {
            "--local" => {
                cli_args.local_model = true;
                consumed_indices.push(i);
                i += 1;
                consumed_current_arg = true;
            }
            "--browser" => {
                browser_flag_found = true;
                if i + 1 < args.len() && !args[i + 1].starts_with("--") {
                    cli_args.browser_url = Some(args[i + 1].clone());
                    consumed_indices.push(i);
                    consumed_indices.push(i + 1);
                    i += 2;
                    consumed_current_arg = true;
                } else {
                    return Err("Missing URL for --browser option".to_string());
                }
            }
            "--input" => {
                if i + 1 < args.len() && !args[i + 1].starts_with("--") {
                    cli_args.input_selector = Some(args[i + 1].clone());
                    consumed_indices.push(i);
                    consumed_indices.push(i + 1);
                    i += 2;
                    consumed_current_arg = true;
                } else {
                    return Err("Missing selector for --input option".to_string());
                }
            }
            "--codeblock" => {
                if i + 1 < args.len() && !args[i + 1].starts_with("--") {
                    cli_args.codeblock_selector = Some(args[i + 1].clone());
                    consumed_indices.push(i);
                    consumed_indices.push(i + 1);
                    i += 2;
                    consumed_current_arg = true;
                } else {
                    return Err("Missing selector for --codeblock option".to_string());
                }
            }
            "--finished" => {
                if i + 1 < args.len() && !args[i + 1].starts_with("--") {
                    cli_args.finished_selector = Some(args[i + 1].clone());
                    consumed_indices.push(i);
                    consumed_indices.push(i + 1);
                    i += 2;
                    consumed_current_arg = true;
                } else {
                    return Err("Missing selector for --finished option".to_string());
                }
            }
            "--verify-with" => {
                if i + 1 < args.len() && !args[i+1].starts_with("--") {
                    cli_args.verify_with = args[i + 1].clone();
                    consumed_indices.push(i);
                    consumed_indices.push(i + 1);
                    i += 2; // Consumed option and its value
                    consumed_current_arg = true;
                } else {
                    return Err("Missing value for --verify-with".to_string());
                }
            }
            "--no-test" => {
                cli_args.no_test = true;
                consumed_indices.push(i);
                i += 1;
                consumed_current_arg = true;
            }
            "--project-root" => {
                if i + 1 < args.len() && !args[i+1].starts_with("--") {
                    cli_args.project_root = PathBuf::from(args[i + 1].clone());
                    consumed_indices.push(i);
                    consumed_indices.push(i + 1);
                    i += 2;
                    consumed_current_arg = true;
                } else {
                    return Err("Missing value for --project-root".to_string());
                }
            }
            "--max-data-loops" => {
                if i + 1 < args.len() && !args[i+1].starts_with("--") {
                    cli_args.max_data_loops = args[i + 1]
                        .parse()
                        .map_err(|e| format!("Invalid value for --max-data-loops: {}", e))?;
                    consumed_indices.push(i);
                    consumed_indices.push(i + 1);
                    i += 2;
                    consumed_current_arg = true;
                } else {
                    return Err("Missing value for --max-data-loops".to_string());
                }
            }
            "--max-verify-retries" => {
                if i + 1 < args.len() && !args[i+1].starts_with("--") {
                    cli_args.max_verify_retries = args[i + 1]
                        .parse()
                        .map_err(|e| format!("Invalid value for --max-verify-retries: {}", e))?;
                    consumed_indices.push(i);
                    consumed_indices.push(i + 1);
                    i += 2;
                    consumed_current_arg = true;
                } else {
                    return Err("Missing value for --max-verify-retries".to_string());
                }
            }
            // Debug mode specifiers
            "debug:initial" => {
                cli_args.debug_mode = Some(DebugMode::Initial);
                consumed_indices.push(i);
                i += 1;
                consumed_current_arg = true;
            }
            "debug:sufficient" => {
                cli_args.debug_mode = Some(DebugMode::Sufficient);
                consumed_indices.push(i);
                i += 1;
                consumed_current_arg = true;
            }
            "debug:changes" => {
                cli_args.debug_mode = Some(DebugMode::Changes);
                consumed_indices.push(i);
                i += 1;
                consumed_current_arg = true;
            }
            _ => {
                // If it's not a recognized option or debug specifier, leave it for Pass 2 (request parts)
                // Only increment i if we didn't consume anything in this match arm.
            }
        }
        if !consumed_current_arg {
            i+=1;
        }
    }

    // Pass 2: Collect request parts from unconsumed arguments
    for (idx, arg_part) in args.iter().enumerate() {
        if !consumed_indices.contains(&idx) {
            request_parts.push(arg_part.clone());
        }
    }
    cli_args.user_request = request_parts.join(" ");

    // Validate browser options first, as they are more specific
    if !browser_flag_found &&
       (cli_args.input_selector.is_some() || cli_args.codeblock_selector.is_some() || cli_args.finished_selector.is_some()) {
        return Err("--input, --codeblock, or --finished options require the --browser option to be specified.".to_string());
    }

    // If --browser flag was found but URL is None (e.g. "--browser --other-option"), it's an error.
    // This is already handled by the parsing logic for --browser itself, but double-check.
    if browser_flag_found && cli_args.browser_url.is_none() {
         return Err("Missing URL for --browser option".to_string());
    }

    // General check for missing user request if not showing help and not in browser mode and not local model mode
    if !cli_args.show_help &&
       cli_args.user_request.is_empty() &&
       cli_args.debug_mode.is_none() &&
       cli_args.browser_url.is_none() &&
       !cli_args.local_model {
        return Err("User request, --browser URL, or --local flag is missing. Use 'gem help' for usage information.".to_string());
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

    --browser <URL>
        URL to open in the browser for web-based tasks.
    --input <SELECTOR>
        CSS selector for the input field (requires --browser).
    --codeblock <SELECTOR>
        CSS selector for the codeblock to copy from (requires --browser).
    --finished <SELECTOR>
        CSS selector to indicate task completion (requires --browser).

    --local
        Use a local model (e.g., Gemma) instead of the Gemini API.

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
        assert!(err_opts_only.contains("User request, --browser URL, or --local flag is missing"));
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

    #[test]
    fn test_all_options_used_together() {
        let args = parse_cli_args(str_vec(&[
            "--project-root",
            "/tmp/xyz",
            "--verify-with",
            "make check",
            "my",
            "request",
            "is this",
            "--no-test",
            "--max-data-loops",
            "10",
            "--max-verify-retries",
            "5",
        ]))
        .unwrap();
        assert_eq!(args.user_request, "my request is this");
        assert_eq!(args.project_root, PathBuf::from("/tmp/xyz"));
        assert_eq!(args.verify_with, "make check");
        assert!(args.no_test);
        assert_eq!(args.max_data_loops, 10);
        assert_eq!(args.max_verify_retries, 5);
        assert_eq!(args.debug_mode, None);
    }

    #[test]
    fn test_options_after_request() {
        // Current parser treats options after request parts as part of the request.
        // The new parser should correctly identify them as options.
        let args = parse_cli_args(str_vec(&[
            "my",
            "request",
            "--no-test",
            "--verify-with",
            "specific_command",
        ]))
        .unwrap();
        assert_eq!(args.user_request, "my request");
        assert!(args.no_test);
        assert_eq!(args.verify_with, "specific_command");
    }

    #[test]
    fn test_debug_mode_with_options_after_request() {
        let args = parse_cli_args(str_vec(&[
            "debug:changes",
            "my",
            "debug",
            "request",
            "--no-test",
            "--max-data-loops",
            "1",
        ]))
        .unwrap();
        assert_eq!(args.debug_mode, Some(DebugMode::Changes));
        assert_eq!(args.user_request, "my debug request");
        assert!(args.no_test);
        assert_eq!(args.max_data_loops, 1);
    }

    #[test]
    fn test_empty_value_for_option_requiring_value() {
        // --verify-with ""
         // The error is not about emptiness but about it being a valid command.
         // An empty string as a command might be valid on some systems or a way to skip.
         // The parser should accept it. If it's an issue, it's for the execution part.
         // Let's re-evaluate this. The parser *should* accept an empty string.
         // The previous error was "Missing value". Providing "" is not missing.
        let args_verify_empty = parse_cli_args(str_vec(&["request", "--verify-with", ""])).unwrap();
        assert_eq!(args_verify_empty.verify_with, "");


        // --project-root ""
        let args_proj_empty = parse_cli_args(str_vec(&["request", "--project-root", ""])).unwrap();
        assert_eq!(args_proj_empty.project_root, PathBuf::from(""));
    }

    #[test]
    fn test_numeric_option_bounds() {
        // Zero for loops/retries
        let args_zero = parse_cli_args(str_vec(&[
            "req",
            "--max-data-loops",
            "0",
            "--max-verify-retries",
            "0",
        ]))
        .unwrap();
        assert_eq!(args_zero.max_data_loops, 0);
        assert_eq!(args_zero.max_verify_retries, 0);

        // Large values (should parse, actual clamping/validation is elsewhere if needed)
        let args_large = parse_cli_args(str_vec(&[
            "req",
            "--max-data-loops",
            "1000",
        ]))
        .unwrap();
        assert_eq!(args_large.max_data_loops, 1000);
    }

    #[test]
    fn test_help_anywhere() {
        let args1 = parse_cli_args(str_vec(&["my", "request", "--help", "--no-test"])).unwrap();
        assert!(args1.show_help);

        let args2 = parse_cli_args(str_vec(&["--no-test", "help", "my", "request"])).unwrap();
        assert!(args2.show_help);
    }
     #[test]
    fn test_empty_request_with_options_is_error() {
        let err = parse_cli_args(str_vec(&["--no-test", "--max-data-loops", "1"])).unwrap_err();
        // Adjusted to new error message
        assert!(err.contains("User request, --browser URL, or --local flag is missing"));
    }

    #[test]
    fn test_debug_specifier_anywhere() {
        // Before options
        let args1 = parse_cli_args(str_vec(&["debug:initial", "--no-test", "req"])).unwrap();
        assert_eq!(args1.debug_mode, Some(DebugMode::Initial));
        assert!(args1.no_test);
        assert_eq!(args1.user_request, "req");

        // After options
        let args2 = parse_cli_args(str_vec(&["--no-test", "debug:sufficient", "req2"])).unwrap();
        assert_eq!(args2.debug_mode, Some(DebugMode::Sufficient));
        assert!(args2.no_test);
        assert_eq!(args2.user_request, "req2");

        // In the middle of request parts (becomes part of request if not first unconsumed item for debug)
        // The current two-pass logic: debug:stage is consumed in pass 1.
        // So, "req1 debug:changes req2" -> debug_mode=Changes, request="req1 req2"
        let args3 = parse_cli_args(str_vec(&["req1", "debug:changes", "req2", "--no-test"])).unwrap();
        assert_eq!(args3.debug_mode, Some(DebugMode::Changes));
        assert!(args3.no_test);
        assert_eq!(args3.user_request, "req1 req2");
    }
}

#[cfg(test)]
mod tests_cli_browser_options {
    use super::*;
    use std::path::PathBuf;

    fn str_vec(args: &[&str]) -> Vec<String> {
        std::iter::once("gem_program_name".to_string())
            .chain(args.iter().map(|s| s.to_string()))
            .collect()
    }

    #[test]
    fn test_parse_browser_with_url() {
        let args = parse_cli_args(str_vec(&["--browser", "https://example.com"])).unwrap();
        assert_eq!(args.browser_url, Some("https://example.com".to_string()));
        assert!(args.user_request.is_empty());
    }

    #[test]
    fn test_parse_browser_with_url_and_request() {
        let args = parse_cli_args(str_vec(&["--browser", "https://example.com", "do", "something", "on", "page"])).unwrap();
        assert_eq!(args.browser_url, Some("https://example.com".to_string()));
        assert_eq!(args.user_request, "do something on page");
    }

    #[test]
    fn test_parse_browser_with_all_options() {
        let args = parse_cli_args(str_vec(&[
            "--browser", "https://example.com",
            "--input", "#inputField",
            "--codeblock", ".code > pre",
            "--finished", "#doneButton",
            "process data"
        ])).unwrap();
        assert_eq!(args.browser_url, Some("https://example.com".to_string()));
        assert_eq!(args.input_selector, Some("#inputField".to_string()));
        assert_eq!(args.codeblock_selector, Some(".code > pre".to_string()));
        assert_eq!(args.finished_selector, Some("#doneButton".to_string()));
        assert_eq!(args.user_request, "process data");
    }

    #[test]
    fn test_parse_browser_with_some_options() {
        let args = parse_cli_args(str_vec(&[
            "--browser", "https://example.com",
            "--input", "textarea.prompt",
            "submit prompt"
        ])).unwrap();
        assert_eq!(args.browser_url, Some("https://example.com".to_string()));
        assert_eq!(args.input_selector, Some("textarea.prompt".to_string()));
        assert_eq!(args.codeblock_selector, None);
        assert_eq!(args.finished_selector, None);
        assert_eq!(args.user_request, "submit prompt");
    }

    #[test]
    fn test_error_browser_without_url() {
        let err = parse_cli_args(str_vec(&["--browser"])).unwrap_err();
        assert_eq!(err, "Missing URL for --browser option");

        // This should also fail as --input is an option, not a URL
        let err2 = parse_cli_args(str_vec(&["--browser", "--input", "#sel"])).unwrap_err();
        assert_eq!(err2, "Missing URL for --browser option");
    }

    #[test]
    fn test_error_input_without_browser() {
        let err = parse_cli_args(str_vec(&["--input", "#inputField", "some request"])).unwrap_err();
        assert_eq!(err, "--input, --codeblock, or --finished options require the --browser option to be specified.");
    }

    #[test]
    fn test_error_codeblock_without_browser() {
        let err = parse_cli_args(str_vec(&["--codeblock", ".code", "some request"])).unwrap_err();
        assert_eq!(err, "--input, --codeblock, or --finished options require the --browser option to be specified.");
    }

    #[test]
    fn test_error_finished_without_browser() {
        let err = parse_cli_args(str_vec(&["--finished", "#done", "some request"])).unwrap_err();
        assert_eq!(err, "--input, --codeblock, or --finished options require the --browser option to be specified.");
    }

    #[test]
    fn test_no_browser_args_parses_existing_correctly() {
        let args = parse_cli_args(str_vec(&["regular", "request", "--no-test"])).unwrap();
        assert_eq!(args.user_request, "regular request");
        assert!(args.no_test);
        assert_eq!(args.browser_url, None);
        assert_eq!(args.input_selector, None);
    }

    #[test]
    fn test_existing_cli_args_with_browser_args() {
        let args = parse_cli_args(str_vec(&[
            "--project-root", "/path/to/proj",
            "--browser", "https://example.com",
            "--input", "#myInput",
            "perform web task",
            "--no-test"
        ])).unwrap();
        assert_eq!(args.project_root, PathBuf::from("/path/to/proj"));
        assert_eq!(args.browser_url, Some("https://example.com".to_string()));
        assert_eq!(args.input_selector, Some("#myInput".to_string()));
        assert_eq!(args.user_request, "perform web task");
        assert!(args.no_test);
    }

    #[test]
    fn test_browser_options_interspersed_with_request_parts() {
        // The current parser design consumes known options greedily.
        // So, "request_part1 --browser http://a.co request_part2 --input #in"
        // will correctly parse --browser and --input, and remaining parts form the request.
        let args = parse_cli_args(str_vec(&[
            "part1",
            "--browser", "http://example.com",
            "part2",
            "--input", "input#id",
            "part3"
        ])).unwrap();
        assert_eq!(args.browser_url, Some("http://example.com".to_string()));
        assert_eq!(args.input_selector, Some("input#id".to_string()));
        assert_eq!(args.user_request, "part1 part2 part3");
    }
     #[test]
    fn test_ensure_url_is_not_an_option() {
        // Test that --browser doesn't consume a subsequent known option as its URL
        let err = parse_cli_args(str_vec(&["--browser", "--no-test"])).unwrap_err();
        assert_eq!(err, "Missing URL for --browser option");

        // Test that --browser does not consume another browser option as its URL
        let err2 = parse_cli_args(str_vec(&["--browser", "--input"])).unwrap_err();
        assert_eq!(err2, "Missing URL for --browser option");
    }
}

#[cfg(test)]
mod tests_cli_local_option {
    use super::*;

    fn str_vec(args: &[&str]) -> Vec<String> {
        std::iter::once("gem_program_name".to_string())
            .chain(args.iter().map(|s| s.to_string()))
            .collect()
    }

    #[test]
    fn test_parse_local_flag() {
        let args = parse_cli_args(str_vec(&["--local", "summarize this"])).unwrap();
        assert!(args.local_model);
        assert_eq!(args.user_request, "summarize this");
    }

    #[test]
    fn test_local_flag_defaults_to_false() {
        let args = parse_cli_args(str_vec(&["regular request"])).unwrap();
        assert!(!args.local_model);
    }

    #[test]
    fn test_local_flag_with_other_args() {
        let args = parse_cli_args(str_vec(&[
            "--local",
            "--project-root", "/tmp",
            "process data locally"
        ])).unwrap();
        assert!(args.local_model);
        assert_eq!(args.project_root, PathBuf::from("/tmp"));
        assert_eq!(args.user_request, "process data locally");
    }

    #[test]
    fn test_local_flag_takes_precedence_over_browser() {
        // This test ensures that if --local is present, browser args are parsed but main logic should ignore them.
        // The actual precedence is handled in main.rs, parser just parses what's given.
        let args = parse_cli_args(str_vec(&[
            "--local",
            "--browser", "http://example.com",
            "--input", "#input",
            "local task"
        ])).unwrap();
        assert!(args.local_model);
        assert_eq!(args.browser_url, Some("http://example.com".to_string())); // Parsed
        assert_eq!(args.input_selector, Some("#input".to_string())); // Parsed
        assert_eq!(args.user_request, "local task");
    }

    #[test]
    fn test_local_flag_alone_is_sufficient_input() {
        let args = parse_cli_args(str_vec(&["--local"])).unwrap();
        assert!(args.local_model);
        assert!(args.user_request.is_empty()); // User request can be empty if --local is provided
    }
}

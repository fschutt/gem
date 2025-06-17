use std::env;
use std::path::PathBuf; // Keep for project_root canonicalization
use std::process; // For process::exit

// Use items from our library crate "gem"
use gem::cli::{self, CustomCliArgs}; // cli module from lib
use gem::cache::Session;             // cache module from lib
use gem::llm_api::{LLMApi, RealLLMApi}; // llm_api module from lib
use gem::run_gem_agent;               // The main logic function

// Type alias for Result, specific to main, or use gem::Result
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;


fn main() -> Result<()> {
    let is_interactive = atty::is(atty::Stream::Stdout);
    // ProgressBar is now handled within run_gem_agent

    let raw_args: Vec<String> = env::args().collect();
    let args = match cli::parse_cli_args(raw_args) { // This should use gem::cli::parse_cli_args
        Ok(a) => a,
        Err(e) => {
            eprintln!("Error: {}", e);
            cli::print_custom_help(); // This should use gem::cli::print_custom_help
            process::exit(1);
        }
    };

    if args.show_help {
        cli::print_custom_help(); // This should use gem::cli::print_custom_help
        return Ok(());
    }

    let project_root = args.project_root.canonicalize().map_err(|e| {
        format!(
            "Failed to canonicalize project root {:?}: {}",
            args.project_root, e
        )
    })?;

    // Initial messages (simplified in main, more detail in run_gem_agent if interactive)
    if is_interactive {
        println!("gem: Starting session..."); // General startup message
        println!("gem: Project root: {:?}", project_root);
        println!("gem: User request: \"{}\"", args.user_request);

    } else {
        // Non-interactive might want less verbose initial output or structured logging
        println!("Project root: {:?}", project_root);
        println!("User request: \"{}\"", args.user_request);
    }

    let gemini_api_key = if args.debug_mode.is_some() {
        if is_interactive { println!("gem: DEBUG MODE ACTIVE"); }
        else { println!("DEBUG MODE ACTIVE"); }
        String::new()
    } else {
        match env::var("GEM_KEY") {
            Ok(key) => key,
            Err(_) => {
                println!("gem: GEM_KEY environment variable not set, using fallback key.");
                "AIzaSyDRuWmnT1X7VC_Ur-EkTqr62jrdQ78GDsw".to_string() // Fallback key
            }
        }
    };

    let session_id_str = format!("{:?}", args); // Use all args for session ID uniqueness
    let session_id = Session::compute_hash(&session_id_str);
    if is_interactive { println!("gem: Session ID: {}", session_id); }
    else { println!("Session ID: {}", session_id); }

    let mut session = Session::new(&session_id);

    let llm_api_instance: Box<dyn LLMApi> = Box::new(RealLLMApi::new(gemini_api_key));

    // Call the main agent logic, now in the library
    if let Err(e) = run_gem_agent(args, &mut session, llm_api_instance, is_interactive, project_root) {
        eprintln!("gem: Critical error during agent execution: {}", e);
        process::exit(1);
    }

    Ok(())
}

// All helper functions (check_dependencies, gather_initial_project_info, etc.)
// and specific Gemini response structs (GeminiNeededItemsResponse, etc.)
// and call_real_gemini_api, call_gemini_api_with_session, call_gemini_api_mock
// have been moved to src/lib.rs or src/llm_api.rs within the gem library.

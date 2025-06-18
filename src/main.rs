use anyhow::Result;
use clap::Parser;
use gem::cli::CustomCliArgs; // Assuming your CLI args are defined here
use std::io::{self, IsTerminal, Read};

// Modules for model loading and inference logic
mod gemma;
mod inference;

#[tokio::main]
async fn main() -> Result<()> {
    // Parse command line arguments using CustomCliArgs
    let args = CustomCliArgs::parse();

    // Determine the user's request from arguments or stdin
    let user_request = if !args.user_request_parts.is_empty() {
        args.user_request_parts.join(" ")
    } else if !io::stdin().is_terminal() {
        // Check if stdin is being piped
        let mut buffer = String::new();
        io::stdin().read_to_string(&mut buffer)?;
        buffer.trim().to_string() // Trim whitespace, especially trailing newlines
    } else {
        // No user request provided via argument or stdin pipe
        eprintln!("Error: No user request provided. Please provide a request via arguments or pipe it through stdin.");
        std::process::exit(1);
    };

    if user_request.is_empty() {
        eprintln!("Error: User request is empty.");
        std::process::exit(1);
    }

    println!("Processing request: {}", user_request);

    if args.local_model {
        #[cfg(feature = "mistral_integration")]
        {
            // Mistral local inference path
            match inference::InferenceEngine::initialize().await {
                Ok(_) => {
                    println!("Mistral Inference engine initialized.");
                    match inference::InferenceEngine::generate_text(&user_request, 100).await {
                        Ok(response) => {
                            println!("\nLLM Response (Mistral):\n{}", response);
                        }
                        Err(e) => {
                            eprintln!("Error generating LLM response from Mistral: {}", e);
                            std::process::exit(1);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Error initializing Mistral inference engine: {}", e);
                    std::process::exit(1);
                }
            }
        }
        #[cfg(not(feature = "mistral_integration"))]
        {
            // Gemma local inference path
            println!("Mistral integration not built. Using local Gemma model.");
            match gemma::run_local_gemma(&user_request) {
                Ok(response) => {
                    println!("\nLLM Response (Gemma):\n{}", response);
                }
                Err(e) => {
                    eprintln!("Error generating LLM response from Gemma: {}", e);
                    std::process::exit(1);
                }
            }
        }
    } else if args.browser_url.is_some() {
        // Browser interaction path
        println!("Browser URL specified. Initiating browser interaction.");

        // Extract necessary arguments for handle_browser_request
        // The URL is guaranteed to be Some by the if condition.
        let url = args.browser_url.as_ref().unwrap(); // Safe due to the if condition
        let input_selector = args.input_selector.as_deref();
        let codeblock_selector = args.codeblock_selector.as_deref();
        let finished_selector = args.finished_selector.as_deref();

        match gem::browser_interaction::execute_browser_interaction_task(
            url,
            input_selector,
            codeblock_selector,
            finished_selector,
            &user_request, // user_request is already prepared
        ) {
            Ok(code_blocks) => {
                if code_blocks.is_empty() {
                    println!("Browser interaction completed. No code blocks were extracted.");
                } else {
                    println!("Browser interaction completed. Extracted code blocks:");
                    for (i, block) in code_blocks.iter().enumerate() {
                        println!("\n--- Code Block {} ---", i + 1);
                        println!("{}", block);
                        println!("--- End Code Block {} ---", i + 1);
                    }
                }
            }
            Err(e) => {
                eprintln!("Browser interaction failed: {}", e);
                std::process::exit(1);
            }
        }
    } else {
        // Default to Gemini HTTP API via run_gem_agent
        println!("Using Gemini HTTP API via run_gem_agent.");
        let gemini_api_key = std::env::var("GEMINI_API_KEY")
            .map_err(|e| {
                eprintln!("Error: GEMINI_API_KEY environment variable not set or accessible.");
                eprintln!("Please set GEMINI_API_KEY to use the Gemini API.");
                eprintln!("Details: {}", e);
                anyhow::anyhow!("GEMINI_API_KEY not found: {}", e) // Return an error that can be propagated or handled
            })?;

        if gemini_api_key.is_empty() {
            eprintln!("Error: GEMINI_API_KEY is set but empty.");
            eprintln!("Please ensure GEMINI_API_KEY has a valid value.");
            std::process::exit(1);
        }

        let llm_api = gem::llm_api::RealLLMApi::new(gemini_api_key);

        // Create a session ID based on the user request.
        let session_id_str = format!("gem_session_{}", user_request.chars().take(20).collect::<String>());
        let session_id = gem::cache::Session::compute_hash(&session_id_str);
        let mut session = gem::cache::Session::new(&session_id);

        let is_interactive = io::stdout().is_terminal();

        // `args.project_root` is already a PathBuf.
        // Clone project_root before moving args.
        let project_root_clone = args.project_root.clone();
        match gem::run_gem_agent(args, &mut session, Box::new(llm_api), is_interactive, project_root_clone) {
            Ok(_) => println!("Gem agent finished successfully."),
            Err(e) => {
                eprintln!("Gem agent failed: {}", e);
                std::process::exit(1);
            }
        }
    }
    Ok(())
}

use anyhow::Result;
use clap::Parser;
use gem::cli::CustomCliArgs; // Assuming your CLI args are defined here
use std::io::{self, IsTerminal, Read};

// Modules for model loading and inference logic
mod inference;
// mod model; // Removed as src/model.rs is deleted

#[tokio::main]
async fn main() -> Result<()> {
    // Parse command line arguments using CustomCliArgs
    let args = CustomCliArgs::parse();

    // Determine the user's request:
    // 1. From --user_request argument
    // 2. From piped stdin
    // 3. (Future) From interactive input or more complex sources
    let user_request = if !args.user_request_parts.is_empty() {
        args.user_request_parts.join(" ")
    } else if !io::stdin().is_terminal() {
        // Check if stdin is being piped
        let mut buffer = String::new();
        io::stdin().read_to_string(&mut buffer)?;
        buffer.trim().to_string() // Trim whitespace, especially trailing newlines
    } else {
        // If no argument and no pipe, you might want to error, prompt, or use a default.
        // For now, let's error if no request is provided.
        eprintln!("Error: No user request provided via --user_request or stdin pipe.");
        std::process::exit(1);
    };

    if user_request.is_empty() {
        eprintln!("Error: User request is empty.");
        std::process::exit(1);
    }

    println!("Processing request: {}", user_request);

    // Call the inference function with the user's prompt
    // match inference::generate_text_from_prompt(&user_request).await {
    //     Ok(response) => {
    //         println!("\nLLM Response:\n{}", response);
    //     }
    //     Err(e) => {
    //         eprintln!("Error generating LLM response: {}", e);
    //         // Consider more specific error handling or exit codes if needed
    //         std::process::exit(1);
    //     }
    // }
    // println!("\nLLM Response generation is temporarily disabled due to mistralrs dependency issues."); // Re-enable actual call
    // Call the inference function with the user's prompt
    match inference::InferenceEngine::initialize().await {
        Ok(_) => {
            println!("Inference engine initialized.");
            match inference::InferenceEngine::generate_text(&user_request, 100).await { // Using 100 tokens for example
                Ok(response) => {
                    println!("\nLLM Response:\n{}", response);
                }
                Err(e) => {
                    eprintln!("Error generating LLM response: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Err(e) => {
            eprintln!("Error initializing inference engine: {}", e);
            std::process::exit(1);
        }
    }

    // Placeholder for where the rest of the gem agent logic would go,
    // e.g., parsing the LLM response, applying changes, verification.
    // For now, this main.rs focuses on getting and printing the LLM response.
    // Example:
    // if let Some(project_root) = args.project_root {
    //     let mut session = gem::cache::Session::new("default_session"); // Example session
    //     let llm_api = gem::llm_api::MistralLLMApi::new(); // Or your new Mistral API wrapper
    //
    //     match gem::run_gem_agent(args, &mut session, Box::new(llm_api), true, project_root.into()) {
    //         Ok(_) => println!("Gem agent finished successfully."),
    //         Err(e) => eprintln!("Gem agent failed: {}", e),
    //     }
    // } else {
    //     println!("Project root not specified, skipping agent run.");
    // }

    Ok(())
}

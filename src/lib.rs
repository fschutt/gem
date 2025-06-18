// Declare modules that are part of the library
pub mod llm_api;
pub mod cache;
pub mod cli;
pub mod parser;
pub mod locatesource;
pub mod browser_interaction;
pub mod gemma;
pub mod llm_response_parser;

// Standard library imports needed by moved functions
use std::collections::HashMap;
use std::fs; // Added for file operations
use std::path::{Path, PathBuf};
use std::process::Command;
use syn; // Added for parsing Rust code content
use std::time::Duration; // For ProgressBar and potentially for reqwest timeout if not already in llm_api

// Crate-local imports (modules defined above)
use cache::Session;
use cli::CustomCliArgs; // Used for structuring command line arguments.
use llm_api::LLMApi; // Use the trait

// Re-export types needed for integration tests and by the binary crate
pub use llm_api::{
    GeminiNeededItemsResponse,
    GeminiSufficiencyResponse,
    GeminiCodeGenerationResponse,
    CodeChange,
    CodeChangeAction,
    TestChange,
    RealLLMApi, // Re-exporting for main
    MockLLMApi // Re-exporting for tests
};
// No need to re-export LLMApi trait if it's only used internally by run_gem_agent's signature here
// and then implemented by RealLLMApi / MockLLMApi. But tests might want it.
pub use llm_api::LLMApi as ExtLLMApi; // Alias if there's a naming conflict concern

pub use cache::Session as ExtSession; // Alias if needed
pub use cli::CustomCliArgs as ExtCustomCliArgs;


// External crate imports needed by moved functions
use ignore::WalkBuilder;
use indicatif::{ProgressBar, ProgressStyle}; // If run_gem_agent handles ProgressBar

// --- Global Constants (moved from main.rs) ---
// These might need to be public if prompts use them and prompts are constructed outside.
// For now, assuming prompts are constructed within this lib.
const FLASH_MODEL_NAME: &str = "gemini-2.5-flash-preview-04-17";
const THINKING_MODEL_NAME: &str = "gemini-2.5-flash-preview-04-17";


// --- Error Type ---
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

// --- Main Agent Logic (moved from main.rs) ---
pub fn run_gem_agent(
    args: CustomCliArgs, // Use the one from cli.rs, make mutable if user_request is joined into it
    session: &mut Session, // Use the one from cache.rs
    llm_api: Box<dyn LLMApi>, // Use the trait from llm_api.rs
    is_interactive: bool,
    project_root: PathBuf,
) -> Result<()> {
    let mut pb: Option<ProgressBar> = None;

    if is_interactive {
        pb = Some(ProgressBar::new_spinner());
        pb.as_ref().unwrap().set_style(ProgressStyle::default_spinner().template("{spinner:.green} {msg}").unwrap());
        pb.as_ref().unwrap().set_message("Checking dependencies (cargo, rustc, rust-analyzer)...");
        pb.as_ref().unwrap().enable_steady_tick(Duration::from_millis(100));
    }
    check_dependencies(&project_root)?;
    if let Some(p) = &pb { p.finish_with_message("Dependencies OK."); }

    if is_interactive {
        pb = Some(ProgressBar::new_spinner());
        pb.as_ref().unwrap().set_style(ProgressStyle::default_spinner().template("{spinner:.green} {msg}").unwrap());
        pb.as_ref().unwrap().set_message("Phase 1: Initial Information Gathering...");
        pb.as_ref().unwrap().enable_steady_tick(Duration::from_millis(100));
    }
    let initial_context = gather_initial_project_info(&project_root)?;
    if let Some(p) = &pb { p.finish_with_message("Initial information gathered."); }

    // Construct user_request from user_request_parts
    let user_request_str = args.user_request_parts.join(" ");
    // Note: user_request_str is constructed here. If CustomCliArgs were to have a processed
    // `user_request: String` field, this could be simplified. For now, this approach is fine.

    let first_prompt = construct_first_gemini_prompt(&user_request_str, &initial_context);

    if args.debug_mode == Some(crate::cli::DebugMode::Initial) { // Assuming cli::DebugMode is accessible
        println!("\n--- DEBUG: INITIAL PROMPT ---");
        println!("{}", first_prompt);
        println!("--- END DEBUG: INITIAL PROMPT ---");
        return Ok(());
    }

    if is_interactive {
        pb = Some(ProgressBar::new_spinner());
        pb.as_ref().unwrap().set_style(ProgressStyle::default_spinner().template("{spinner:.green} {msg}").unwrap());
        pb.as_ref().unwrap().set_message("Asking Gemini what information it needs...");
        pb.as_ref().unwrap().enable_steady_tick(Duration::from_millis(100));
    }
    session.overwrite_prompt("initial", &first_prompt)?;
    let gemini_response_str_result = call_gemini_api_with_session(
        session,
        llm_api.as_ref(),
        "initial",
        &first_prompt,
        FLASH_MODEL_NAME,
    );
    if let Some(p) = &pb { p.finish_and_clear(); }
    let gemini_response_str = clean_gemini_api_json(gemini_response_str_result?);

    let needed_items_response: GeminiNeededItemsResponse =
        serde_json::from_str(&gemini_response_str)?;
    let mut current_needed_items = needed_items_response.needed_items;

    let mut gathered_data_for_gemini: HashMap<String, String> = session.gathered_data.clone();
    let mut data_gathering_iterations = 0;
    let mut verification_attempt = 0;
    let _code_change_attempt = 0;

    loop { // Sufficiency Loop
        if data_gathering_iterations >= args.max_data_loops {
            eprintln!("gem: ERROR: Exceeded maximum data gathering iterations ({}). Giving up.", args.max_data_loops);
            return Err("Max data gathering iterations reached.".into());
        }
        data_gathering_iterations += 1;

        if !current_needed_items.is_empty() {
            if is_interactive {
                pb = Some(ProgressBar::new_spinner());
                pb.as_ref().unwrap().set_style(ProgressStyle::default_spinner().template("{spinner:.green} {msg}").unwrap());
                pb.as_ref().unwrap().set_message(format!("Extracting {} item(s)...", current_needed_items.len()));
                pb.as_ref().unwrap().enable_steady_tick(Duration::from_millis(100));
            }
        }

        for item_path_or_qname in &current_needed_items {
            if !gathered_data_for_gemini.contains_key(item_path_or_qname) {
                if let Some(p) = &pb { p.set_message(format!("Extracting: {}...", item_path_or_qname)); }
                match query_rust_analyzer_for_item_definition(&project_root, item_path_or_qname) {
                    Ok(content) => {
                        gathered_data_for_gemini.insert(item_path_or_qname.clone(), content.clone());
                        session.add_data(item_path_or_qname, &content);
                    }
                    Err(e) => {
                        let error_msg = format!("// GEM_NOTE: Error querying for {}: {}", item_path_or_qname, e);
                        if let Some(p) = &pb { p.println(format!("gem: ERROR: {}", error_msg)); }
                        else { eprintln!("gem: ERROR: {}", error_msg); }
                        gathered_data_for_gemini.insert(item_path_or_qname.clone(), error_msg.clone());
                        session.add_data(item_path_or_qname, &error_msg);
                    }
                }
            }
        }
        if let Some(p) = &pb {
            if !current_needed_items.is_empty() {
                p.finish_with_message("Items extracted.");
            } else {
                p.finish_and_clear();
            }
        }
        // pb = None; // Reset progress bar after this section // This line is removed as pb is reassigned or goes out of scope

        session.save()?;
        current_needed_items.clear();

        let user_request_str = args.user_request_parts.join(" "); // Reconstruct here too or pass around
        let sufficiency_prompt = construct_sufficiency_check_prompt(&user_request_str, &gathered_data_for_gemini);
        session.append_to_prompt("sufficient", &sufficiency_prompt)?;

        if args.debug_mode == Some(crate::cli::DebugMode::Sufficient) {
            println!("\n--- DEBUG: SUFFICIENCY PROMPT (Iteration {}) ---", data_gathering_iterations);
            println!("{}", sufficiency_prompt);
            println!("--- END DEBUG: SUFFICIENCY PROMPT ---");
            return Ok(());
        }

        if is_interactive {
            pb = Some(ProgressBar::new_spinner());
            pb.as_ref().unwrap().set_style(ProgressStyle::default_spinner().template("{spinner:.green} {msg}").unwrap());
            pb.as_ref().unwrap().set_message("Asking Gemini if gathered data is sufficient...");
            pb.as_ref().unwrap().enable_steady_tick(Duration::from_millis(100));
        }

        let gemini_sufficiency_response_str_result = if let Some(cached_response) =
            session.get_cached_response("sufficient", &sufficiency_prompt)
        {
            if is_interactive { if let Some(p) = &pb {p.println("gem: Using cached sufficiency response");} }
            else { println!("Using cached sufficiency response"); }
            Ok(cached_response)
        } else {
            let res = call_gemini_api_with_session(
                session,
                llm_api.as_ref(),
                "sufficient",
                &sufficiency_prompt,
                FLASH_MODEL_NAME,
            );
            res
        };

        if let Some(p) = &pb { p.finish_and_clear(); }
        let cleaned_response = clean_gemini_api_json(gemini_sufficiency_response_str_result?);
        let sufficiency_response: GeminiSufficiencyResponse = serde_json::from_str(&cleaned_response)?;

        if sufficiency_response.sufficient { break; }
        else {
            if !sufficiency_response.needed_items.is_empty() {
                current_needed_items.extend(sufficiency_response.needed_items.into_iter().filter(|item| !gathered_data_for_gemini.contains_key(item)));
                if current_needed_items.is_empty() && !gathered_data_for_gemini.is_empty() {
                    if is_interactive { if let Some(p) = &pb {p.println("gem: All newly requested items were already gathered. Proceeding to check sufficiency again.");} else {println!("gem: All newly requested items were already gathered. Proceeding to check sufficiency again.");} }
                     else { println!("gem: All newly requested items were already gathered. Proceeding to check sufficiency again."); }
                }
            } else {
                eprintln!("gem: ERROR: Gemini reported data is not sufficient but did not specify what's needed. Giving up.");
                return Err("Gemini insufficient without item list.".into());
            }
        }
    }

    let mut verification_failures_context = String::new();
    loop { // Code Generation Loop
        if verification_attempt >= args.max_verify_retries + 1 {
            eprintln!("gem: ERROR: Exceeded maximum verification retries ({}). Giving up.", args.max_verify_retries);
            eprintln!("gem: The last unverified changes might be committed. Please review your git history.");
            return Err("Max verification retries reached.".into());
        }
        verification_attempt += 1;
        if !is_interactive {
            println!("\ngem: Phase 3: Code Generation & Verification Attempt {}/{}...", verification_attempt, args.max_verify_retries + 1);
        }

        let user_request_str = args.user_request_parts.join(" "); // Reconstruct here too or pass around
        let code_gen_prompt = construct_code_generation_prompt(&user_request_str, &gathered_data_for_gemini, !args.no_test, if verification_attempt > 1 { Some(&verification_failures_context) } else { None }, &args.verify_with);
        session.append_to_prompt("change", &code_gen_prompt)?;

        if args.debug_mode == Some(crate::cli::DebugMode::Changes) && verification_attempt == 1 {
            println!("\n--- DEBUG: CODE GENERATION PROMPT (Attempt 1) ---");
            println!("{}", code_gen_prompt);
            println!("--- END DEBUG: CODE GENERATION PROMPT ---");
            return Ok(());
        }

        if is_interactive {
            pb = Some(ProgressBar::new_spinner());
            pb.as_ref().unwrap().set_style(ProgressStyle::default_spinner().template("{spinner:.green} {msg}").unwrap());
            pb.as_ref().unwrap().set_message(format!("Asking Gemini for code (attempt {})...", verification_attempt));
            pb.as_ref().unwrap().enable_steady_tick(Duration::from_millis(100));
        }

        let gemini_code_gen_response_str_result = if let Some(cached_response) = session.get_cached_response("change", &code_gen_prompt) {
             if is_interactive { if let Some(p) = &pb {p.println("gem: Using cached code generation response");} }
             else { println!("Using cached code generation response"); }
            Ok(cached_response)
        } else {
            let res = call_gemini_api_with_session(
                session,
                llm_api.as_ref(),
                "change",
                &code_gen_prompt,
                THINKING_MODEL_NAME,
            );
            res
        };
        if let Some(p) = &pb { p.finish_and_clear(); }
        let mut code_gen_response: GeminiCodeGenerationResponse = serde_json::from_str(&gemini_code_gen_response_str_result?)?;

        // If ProcessMarkdownAndApplyChanges is used, extract explanation from the markdown content.
        for change in &code_gen_response.changes {
            if change.action == CodeChangeAction::ProcessMarkdownAndApplyChanges {
                if let Some(markdown_content) = &change.content {
                    match crate::llm_response_parser::extract_explanation_from_markdown(markdown_content) {
                        Ok(extracted_explanation) => {
                            if !extracted_explanation.is_empty() {
                                // Override or supplement the existing explanation
                                code_gen_response.explanation = extracted_explanation;
                                // If multiple markdown changes, this will take the last one.
                                // Consider concatenation or other strategies if needed.
                            }
                        }
                        Err(e) => {
                            // Log the error but don't fail the whole process
                            eprintln!("gem: WARN: Failed to extract explanation from markdown: {}", e);
                        }
                    }
                }
            }
        }

        if is_interactive { if let Some(p) = &pb {p.println(format!("gem: Gemini proposes changes: {}", code_gen_response.explanation));} }
        else { println!("Gemini proposes changes: {}", code_gen_response.explanation); }

        if is_interactive {
            pb = Some(ProgressBar::new_spinner());
            pb.as_ref().unwrap().set_style(ProgressStyle::default_spinner().template("{spinner:.green} {msg}").unwrap());
            pb.as_ref().unwrap().set_message("Applying code changes...");
            pb.as_ref().unwrap().enable_steady_tick(Duration::from_millis(100));
        }
        // Call the real apply_code_changes function
        apply_code_changes(&project_root, &code_gen_response.changes)
            .map_err(|e| {
                eprintln!("gem: ERROR: Failed to apply code changes: {}", e);
                // Optionally, provide more context or attempt rollback if applicable
                e
            })?;
        if let Some(p) = &pb { p.finish_with_message("Code changes applied."); }

        if !args.no_test {
            if let Some(tests) = &code_gen_response.tests {
                if is_interactive {
                    pb = Some(ProgressBar::new_spinner());
                    pb.as_ref().unwrap().set_style(ProgressStyle::default_spinner().template("{spinner:.green} {msg}").unwrap());
                    pb.as_ref().unwrap().set_message("Applying tests...");
                    pb.as_ref().unwrap().enable_steady_tick(Duration::from_millis(100));
                }
                apply_tests_mock(&project_root, tests)?;
                if let Some(p) = &pb { p.finish_with_message("Tests applied."); }
            }
        }

        if is_interactive {
            pb = Some(ProgressBar::new_spinner());
            pb.as_ref().unwrap().set_style(ProgressStyle::default_spinner().template("{spinner:.green} {msg}").unwrap());
            pb.as_ref().unwrap().set_message(format!("Verifying changes with: \"{}\" (attempt {})...", args.verify_with, verification_attempt));
            pb.as_ref().unwrap().enable_steady_tick(Duration::from_millis(100));
        }

        match execute_verification_command_mock( &mut verification_attempt, &project_root, &args.verify_with) {
            Ok(output) => {
                if let Some(p) = &pb { p.finish_with_message("Verification successful!"); }
                else { println!("Verification successful!"); }
                println!("Output:\n{}", output);

                let user_request_str = args.user_request_parts.join(" "); // Reconstruct here too or pass around
                let commit_message = format!("gem: Automated change for \"{}\"\n\n{}\n\n", user_request_str, code_gen_response.explanation);
                git_commit_mock(&project_root, &commit_message, verification_attempt > 1)?;
                println!("\ngem: Task completed successfully.");
                return Ok(());
            }
            Err(e) => {
                if let Some(p) = &pb { p.finish_with_message("Verification failed."); }
                else { println!("Verification failed."); }
                verification_failures_context = e.to_string();
                eprintln!("Error Output:\n{}", verification_failures_context);

                if verification_attempt >= args.max_verify_retries + 1 {
                     eprintln!("gem: Max verification retries reached. The last (failed) attempt is committed (amended). Please review and fix manually.");
                    return Err(format!("Verification failed after max retries: {}", verification_failures_context).into());
                }
            }
        }
    }
}

// --- Helper Functions (moved from main.rs, now public for tests) ---
pub fn check_dependencies(_project_root: &Path) -> Result<()> {
    let deps = ["cargo", "rustc", "rust-analyzer"];
    for dep in deps.iter() {
        let dep_cmd = if cfg!(windows) && *dep == "rust-analyzer" { "rust-analyzer.exe" } else { dep };
        match Command::new(dep_cmd).arg("--version").output() {
            Ok(output) if output.status.success() => {}
            Ok(output) => return Err(format!("Dependency '{}' found but '--version' failed. Stderr: {}", dep, String::from_utf8_lossy(&output.stderr)).into()),
            Err(e) => return Err(format!("Dependency '{}' not found or not executable: {}. Please ensure it's installed and in PATH.", dep, e).into()),
        }
    }
    if !atty::is(atty::Stream::Stdout) { // Print only if not interactive (spinner would have shown "Dependencies OK")
        println!("gem: All dependencies found.");
    }
    Ok(())
}

pub fn gather_initial_project_info(project_root: &Path) -> Result<HashMap<String, String>> {
    let mut context = HashMap::new();
    context.insert("rustc_version".to_string(), get_command_output("rustc", &["--version"])?);
    context.insert("cargo_version".to_string(), get_command_output("cargo", &["--version"])?);
    context.insert("rust_analyzer_version".to_string(), get_command_output(if cfg!(windows) { "rust-analyzer.exe" } else { "rust-analyzer" }, &["--version"])?);

    if !atty::is(atty::Stream::Stdout) { // Print only if not interactive
        println!("gem: Gathering project symbols...");
    }
    match crate::parser::get_project_symbols_string(project_root) { // Use crate::parser
        Ok(symbols) => { context.insert("project_symbols".to_string(), symbols); }
        Err(e) => {
            eprintln!("gem: WARN: Failed to parse project symbols: {}", e);
            context.insert("project_symbols".to_string(), "// Failed to parse project symbols".to_string());
        }
    }

    let src_dir = project_root.join("src");
    if src_dir.exists() && src_dir.is_dir() {
        context.insert("src_tree".to_string(), generate_src_tree(&src_dir)?);
    } else {
        context.insert("src_tree".to_string(), "// No src directory found".to_string());
    }
    context.insert("dependencies".to_string(), get_cargo_metadata_dependencies(project_root)?);
    Ok(context)
}

pub fn get_command_output(cmd: &str, args: &[&str]) -> Result<String> {
    let output = Command::new(cmd).args(args).output()?;
    if output.status.success() {
        Ok(String::from_utf8(output.stdout)?.trim().to_string())
    } else {
        Err(format!("Command `{} {:?}` failed: {}", cmd, args, String::from_utf8_lossy(&output.stderr)).into())
    }
}

fn generate_src_tree(src_dir_path: &Path) -> Result<String> {
    if !src_dir_path.exists() || !src_dir_path.is_dir() {
        return Ok(format!("Directory {} not found or is not a directory.", src_dir_path.display()));
    }
    let mut tree_output = String::new();
    let walker = WalkBuilder::new(src_dir_path).hidden(false).parents(true).git_ignore(true).git_global(true).build();
    for result in walker {
        match result {
            Ok(entry) => {
                let path = entry.path();
                if entry.depth() == 0 && path == src_dir_path { continue; }
                let display_path = path.strip_prefix(src_dir_path).unwrap_or(path);
                let indent_level = if entry.depth() > 0 { entry.depth() - 1 } else { 0 };
                let indent = "  ".repeat(indent_level);
                let entry_type_char = if entry.file_type().map_or(false, |ft| ft.is_dir()) { "/" } else { "" };
                tree_output.push_str(&format!("{}{}{}\n", indent, display_path.display(), entry_type_char));
            }
            Err(err) => eprintln!("gem: WARN: error walking directory {} for src_tree: {}", src_dir_path.display(), err),
        }
    }
    Ok(if tree_output.is_empty() { format!("Directory {} is empty or all files are ignored.", src_dir_path.display()) } else { tree_output })
}

fn get_cargo_metadata_dependencies(project_root: &Path) -> Result<String> {
    let output = Command::new("cargo").current_dir(project_root).arg("tree").output()?;
    if !output.status.success() {
        return Err(format!("cargo metadata failed: {}", String::from_utf8_lossy(&output.stderr)).into());
    }
    Ok(String::from_utf8(output.stdout)?)
}

pub fn construct_first_gemini_prompt(user_request: &str, context: &HashMap<String, String>) -> String {
    format!(
        include_str!("prompts/initial.txt"), // Assuming prompts are in src/prompts/ relative to lib.rs
        user_request,
        context.get("rustc_version").unwrap_or(&"N/A".to_string()),
        context.get("cargo_version").unwrap_or(&"N/A".to_string()),
        context.get("rust_analyzer_version").unwrap_or(&"N/A".to_string()),
        context.get("os").unwrap_or(&"N/A".to_string()), // OS info was removed, this will be N/A
        context.get("src_tree").unwrap_or(&"N/A".to_string()),
        context.get("dependencies").unwrap_or(&"{}".to_string()),
        context.get("project_symbols").unwrap_or(&"N/A".to_string())
    )
}

pub fn construct_sufficiency_check_prompt(user_request: &str, gathered_data: &HashMap<String, String>) -> String {
    let mut data_str = String::new();
    for (item, content) in gathered_data { data_str.push_str(&format!("// Item: {}\n// Extracted Code:\n{}\n\n", item, content)); }
    format!(include_str!("prompts/sufficient.txt"), user_request, data_str)
}

pub fn construct_code_generation_prompt(user_request: &str, gathered_data: &HashMap<String, String>, generate_tests: bool, failure_context: Option<&str>, verify_command: &str) -> String {
    let mut data_str = String::new();
    for (item, content) in gathered_data { data_str.push_str(&format!("// Item: {}\n// Extracted Code:\n{}\n\n", item, content)); }
    let test_instruction = if generate_tests { "You should also generate relevant unit tests for the changes." } else { "Test generation is disabled for this request." };
    let failure_prompt_addition = if let Some(ctx) = failure_context { format!(r#"
Previous Attempt Feedback:
The verification command "{}" failed.
Build/Test Output (JSON messages or raw output):
```
{}
```
Please analyze the errors and provide a corrected set of changes and tests.
"#, verify_command, ctx) } else { String::new() };
    format!(include_str!("prompts/change.txt"), test_instruction, user_request, data_str, failure_prompt_addition, test_instruction)
}

// This is the refactored version from the previous step.
fn call_gemini_api_with_session(
    session: &mut Session,
    llm_api: &dyn LLMApi,
    prompt_type: &str,
    prompt_text: &str,
    model_name: &str,
) -> Result<String> {
    if let Some(cached_response) = session.get_cached_response(prompt_type, prompt_text) {
        return Ok(cached_response);
    }
    let response = llm_api.generate_content(prompt_text, model_name)?;
    session.save_prompt_and_response(prompt_type, prompt_text, &response)
        .map_err(|e| format!("Failed to save {} prompt and response: {}", prompt_type, e))?;
    Ok(response)
}

fn clean_gemini_api_json(s: String) -> String {
    let s = if s.trim().starts_with("```json") { s.replacen("```json", "", 1) } else { s };
    let mut t = &s[..];
    if s.ends_with("```") { t = &s[0..(s.len() - 3)]; }
    t.trim().to_string()
}

fn query_rust_analyzer_for_item_definition(project_root: &Path, item_qname_or_path: &str) -> Result<String> {
    match locatesource::retrieve_item_source(project_root, item_qname_or_path) {
        Ok(content) => Ok(content),
        Err(e) => {
            eprintln!("gem: Could not resolve with locatesource: {}", e);
            Err(e.to_string().into())
        }
    }
}

// --- Real Code Change Application ---
pub fn apply_code_changes(project_root: &Path, changes: &[CodeChange]) -> Result<()> {
    for change in changes {
        let full_path = project_root.join(&change.file_path);
        match change.action {
            CodeChangeAction::CreateFile => {
                if let Some(parent_dir) = full_path.parent() {
                    fs::create_dir_all(parent_dir)
                        .map_err(|e| format!("Failed to create parent directories for {:?}: {}", full_path, e))?;
                }
                fs::write(&full_path, change.content.as_deref().unwrap_or(""))
                    .map_err(|e| format!("Failed to create file {:?}: {}", full_path, e))?;
                if !atty::is(atty::Stream::Stdout) { println!("gem: Created file: {:?}", full_path); }

            }
            CodeChangeAction::DeleteFile => {
                fs::remove_file(&full_path)
                    .map_err(|e| format!("Failed to delete file {:?}: {}", full_path, e))?;
                if !atty::is(atty::Stream::Stdout) { println!("gem: Deleted file: {:?}", full_path); }
            }
            CodeChangeAction::ReplaceContent => {
                fs::write(&full_path, change.content.as_deref().unwrap_or(""))
                    .map_err(|e| format!("Failed to replace content of file {:?}: {}", full_path, e))?;
                 if !atty::is(atty::Stream::Stdout) { println!("gem: Replaced content of file: {:?}", full_path); }
            }
            CodeChangeAction::ReplaceItemInSection => {
                // file_path is "path/to/file.rs::ItemName"
                let parts: Vec<&str> = change.file_path.splitn(2, "::").collect();
                if parts.len() != 2 {
                    return Err(format!("Invalid file_path for ReplaceItemInSection: {}. Expected format 'path/to/file.rs::ItemName'", change.file_path).into());
                }
                let actual_file_path_str = parts[0];
                let item_name_suffix = parts[1];
                let actual_file_path = project_root.join(actual_file_path_str);

                let file_content = fs::read_to_string(&actual_file_path)
                    .map_err(|e| format!("Failed to read file {:?} for item replacement: {}", actual_file_path, e))?;

                match parser::find_item_span(&file_content, item_name_suffix, &actual_file_path, project_root) {
                    Ok(Some((start_byte, end_byte))) => {
                        let new_content = format!("{}{}{}",
                            &file_content[..start_byte],
                            change.content.as_deref().unwrap_or(""),
                            &file_content[end_byte..]
                        );
                        fs::write(&actual_file_path, new_content)
                            .map_err(|e| format!("Failed to write updated content to {:?}: {}", actual_file_path, e))?;
                        if !atty::is(atty::Stream::Stdout) { println!("gem: Replaced item '{}' in file: {:?}", item_name_suffix, actual_file_path); }
                    }
                    Ok(None) => {
                        return Err(format!("Failed to find item '{}' in '{}'", item_name_suffix, actual_file_path_str).into());
                    }
                    Err(e) => {
                        return Err(format!("Error finding item span for '{}' in '{}': {}", item_name_suffix, actual_file_path_str, e).into());
                    }
                }
            }
            CodeChangeAction::ProcessMarkdownAndApplyChanges => {
                let markdown_content = change.content.as_deref().unwrap_or_default();
                if change.file_path != "MARKDOWN_CHANGES" && !atty::is(atty::Stream::Stdout) {
                     println!("gem: WARNING: file_path for ProcessMarkdownAndApplyChanges was '{}', expected 'MARKDOWN_CHANGES'. Processing normally.", change.file_path);
                }

                let extracted_blocks = parser::extract_file_code_blocks_from_markdown(markdown_content)
                    .map_err(|e| format!("Failed to parse markdown for code blocks: {}", e))?;

                if extracted_blocks.is_empty() && !atty::is(atty::Stream::Stdout) {
                    println!("gem: WARNING: No file code blocks found in the provided Markdown for ProcessMarkdownAndApplyChanges.");
                }

                for (file_path_str, code_content) in extracted_blocks {
                    let target_file_path = project_root.join(&file_path_str);
                    let mut item_replaced_in_file = false;

                    // Attempt to parse code_content as a single Rust item
                    match syn::parse_str::<syn::Item>(&code_content) {
                        Ok(syn_item) => {
                            let item_name_from_markdown_block = match &syn_item {
                                syn::Item::Const(item) => Some(item.ident.to_string()),
                                syn::Item::Enum(item) => Some(item.ident.to_string()),
                                syn::Item::Fn(item) => Some(item.sig.ident.to_string()),
                                syn::Item::Macro(item) => item.ident.as_ref().map(|id| id.to_string()),
                                syn::Item::Mod(item) => Some(item.ident.to_string()),
                                syn::Item::Static(item) => Some(item.ident.to_string()),
                                syn::Item::Struct(item) => Some(item.ident.to_string()),
                                syn::Item::Trait(item) => Some(item.ident.to_string()),
                                syn::Item::TraitAlias(item) => Some(item.ident.to_string()),
                                syn::Item::Type(item) => Some(item.ident.to_string()),
                                syn::Item::Union(item) => Some(item.ident.to_string()),
                                // syn::Item::ExternCrate, syn::Item::ForeignMod, syn::Item::Impl, syn::Item::Use, syn::Item::Verbatim
                                _ => None, // Not all items have a clear single 'ident' or are suitable for this logic
                            };

                            if let Some(name) = item_name_from_markdown_block {
                                if target_file_path.exists() {
                                    let target_file_content_str = fs::read_to_string(&target_file_path)
                                        .map_err(|e| format!("Failed to read target file {:?} for item replacement: {}", target_file_path, e))?;

                                    match parser::find_item_span(&target_file_content_str, &name, &target_file_path, project_root) {
                                        Ok(Some((start_byte, end_byte))) => {
                                            let new_target_content = format!("{}{}{}",
                                                &target_file_content_str[..start_byte],
                                                &code_content, // Use the full code_content from markdown here
                                                &target_file_content_str[end_byte..]
                                            );
                                            fs::write(&target_file_path, new_target_content)
                                                .map_err(|e| format!("Failed to write updated content to {:?} after item replacement: {}", target_file_path, e))?;
                                            if !atty::is(atty::Stream::Stdout) {
                                                println!("gem: Applied item replacement for '{}' in '{:?}' from Markdown.", name, target_file_path);
                                            }
                                            item_replaced_in_file = true;
                                        }
                                        Ok(None) => { // Item not found in existing file, fall through to whole file write/create
                                            if !atty::is(atty::Stream::Stdout) {
                                                println!("gem: Item '{}' not found in {:?} for in-place replacement. Proceeding with whole file operation.", name, target_file_path);
                                            }
                                        }
                                        Err(e) => { // Error finding span, fall through but log
                                            if !atty::is(atty::Stream::Stdout) {
                                                eprintln!("gem: Error finding item span for '{}' in '{:?}': {}. Proceeding with whole file operation.", name, target_file_path, e);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        Err(_) => { // Not a single parsable item, treat as whole file content.
                            // Optional: log that parsing code_content as single item failed.
                        }
                    }

                    if !item_replaced_in_file {
                        // Fallback: Whole file operation (create or replace)
                        if let Some(parent_dir) = target_file_path.parent() {
                            fs::create_dir_all(parent_dir)
                                .map_err(|e| format!("Failed to create parent directories for extracted file {:?}: {}", target_file_path, e))?;
                        }
                        fs::write(&target_file_path, &code_content) // Ensure code_content is borrowed here
                            .map_err(|e| format!("Failed to write extracted content to file {:?}: {}", target_file_path, e))?;
                        if !atty::is(atty::Stream::Stdout) {
                            println!("gem: Applied (whole file create/replace) from Markdown to file: {:?}", target_file_path);
                        }
                    }
                }
            }
            // Fallback for other actions
            _ => {
                return Err(format!("CodeChangeAction {:?} not yet implemented.", change.action).into());
            }
        }
    }
    Ok(())
}

// Mock implementations (kept in lib.rs for now, might move to tests or test utils later)

fn apply_tests_mock(_project_root: &Path, tests: &[TestChange]) -> Result<()> {
    for test in tests {
        // println!("MOCK: Applying test: {:?}", test);
    }
    Ok(())
}

fn git_commit_mock(_project_root: &Path, _message: &str, _amend: bool) -> Result<()> {
    // println!("MOCK: git commit --message \"{}\" {}", message, if amend { "--amend" } else { "" });
    Ok(())
}

fn execute_verification_command_mock(attempt: &mut usize, _project_root: &Path, command_str: &str) -> Result<String> {
    *attempt += 1; // This mock behavior might need to be controllable for tests
    if *attempt == 1 && command_str.contains("cargo") {
        Err("Mock: Verification FAILED (first attempt)".into())
    } else {
        Ok("Mock: Build successful! All checks passed.".to_string())
    }
}

// The call_gemini_api_mock function was here, but has been removed as it was unused
// and marked as problematic for library use. Mocking is now primarily handled by
// MockLLMApi in tests.

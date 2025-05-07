use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

// For walkdir and ignore
use ignore::WalkBuilder;

// Bring in CLI parsing from cli.rs
mod cli;
mod locatesource;
mod parser;

// --- Gemini API Model Names ---
const THINKING_MODEL_NAME: &str = "gemini-2.5-flash-preview-04-17"; // Example for complex tasks
const FLASH_MODEL_NAME: &str = "gemini-2.5-flash-preview-04-17"; // Example for faster, simpler tasks

// --- Gemini API Request/Response Structures (for real API calls) ---
#[derive(Serialize)]
struct GeminiRequestPart {
    text: String,
}

#[derive(Serialize)]
struct GeminiRequestContent {
    parts: Vec<GeminiRequestPart>,
    #[serde(skip_serializing_if = "Option::is_none")]
    role: Option<String>, // "user" or "model"
}

#[derive(Serialize)]
struct GeminiRequest {
    contents: Vec<GeminiRequestContent>,
    // Can add generationConfig, safetySettings etc. here if needed
}

#[derive(Deserialize, Debug)]
struct GeminiResponsePart {
    text: String,
}

#[derive(Deserialize, Debug)]
struct GeminiResponseContent {
    parts: Vec<GeminiResponsePart>,
    role: String,
}

#[derive(Deserialize, Debug)]
struct GeminiResponseCandidate {
    content: GeminiResponseContent,
    #[serde(alias = "finishReason")] // Handle potential casing differences
    finish_reason: Option<String>,
    // safety_ratings: Vec<...>,
}

#[derive(Deserialize, Debug)]
struct GeminiResponse {
    candidates: Vec<GeminiResponseCandidate>,
    // prompt_feedback: Option<...>,
}

// --- Mock Gemini API Structures (unchanged from original) ---
#[derive(Serialize, Deserialize, Debug)]
struct GeminiNeededItemsResponse {
    needed_items: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug)]
struct GeminiSufficiencyResponse {
    sufficient: bool,
    #[serde(default)]
    needed_items: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
enum CodeChangeAction {
    ReplaceContent,
    CreateFile,
    DeleteFile,
    ApplyDiff,
    ReplaceLines,
    InsertAfterLine,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct CodeChange {
    file_path: String,
    action: CodeChangeAction,
    content: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct TestChange {
    file_path: String,
    action: String,
    content: String,
    test_name: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
struct GeminiCodeGenerationResponse {
    changes: Vec<CodeChange>,
    tests: Option<Vec<TestChange>>,
    explanation: String,
}

// --- Error Type ---
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn main() -> Result<()> {
    let raw_args: Vec<String> = env::args().collect();
    let args = match cli::parse_cli_args(raw_args) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Error: {}", e);
            cli::print_custom_help();
            std::process::exit(1);
        }
    };

    if args.show_help {
        cli::print_custom_help();
        return Ok(());
    }

    let project_root = args.project_root.canonicalize().map_err(|e| {
        format!(
            "Failed to canonicalize project root {:?}: {}",
            args.project_root, e
        )
    })?;

    println!("gem: Creating initial prompt with task and system info...");
    println!("gem: Project root: {:?}", project_root);
    println!("gem: Verification command: \"{}\"", args.verify_with);

    if args.no_test {
        println!("gem: Test generation disabled.");
    }

    let gemini_api_key = if let Some(mode) = args.debug_mode {
        println!("gem: DEBUG MODE ACTIVE: {}", mode);
        String::new()
    } else {
        env::var("GEM_KEY").map_err(|_| "GEM_KEY environment variable not set.")?
    };

    // --- 0. Environment and Dependency Checks ---
    check_dependencies(&project_root)?;

    // --- 1. Initial Information Gathering ---
    println!("\ngem: Phase 1: Initial Information Gathering...");
    let initial_context = gather_initial_project_info(&project_root)?;
    let first_prompt = construct_first_gemini_prompt(&args.user_request, &initial_context);

    if args.debug_mode == Some(crate::cli::DebugMode::Initial) {
        println!("\n--- DEBUG: INITIAL PROMPT ---");
        println!("{}", first_prompt);
        println!("--- END DEBUG: INITIAL PROMPT ---");
        return Ok(());
    }

    // --- First Gemini Call: What data is needed? ---
    println!("gem: Asking Gemini what information it needs...");
    let gemini_response_str = clean_gemini_api_json(call_real_gemini_api(
        &gemini_api_key,
        &first_prompt,
        FLASH_MODEL_NAME,
    )?);
    let _ = std::fs::write("./flash_first_response.txt", &gemini_response_str);
    let needed_items_response: GeminiNeededItemsResponse =
        serde_json::from_str(&gemini_response_str)?;

    let mut current_needed_items = needed_items_response.needed_items;

    // --- 2 & 3. Focused Source Code Extraction & Sufficiency Check Loop ---
    let mut gathered_data_for_gemini: HashMap<String, String> = HashMap::new();
    let mut data_gathering_iterations = 0;
    let mut verification_attempt = 0;
    let mut code_change_attempt = 0;

    loop {
        if data_gathering_iterations >= args.max_data_loops {
            eprintln!(
                "gem: ERROR: Exceeded maximum data gathering iterations ({}). Giving up.",
                args.max_data_loops
            );
            return Err("Max data gathering iterations reached.".into());
        }
        data_gathering_iterations += 1;
        println!("\n");

        if current_needed_items.is_empty() && !gathered_data_for_gemini.is_empty() {
            println!("gem: No new items requested, but previous data was gathered. Assuming sufficient or Gemini will re-request.");
        }

        for item_path_or_qname in &current_needed_items {
            if !gathered_data_for_gemini.contains_key(item_path_or_qname) {
                match query_rust_analyzer_for_item_definition(&project_root, item_path_or_qname) {
                    Ok(Some(content)) => {
                        gathered_data_for_gemini.insert(item_path_or_qname.clone(), content);
                    }
                    Ok(None) => {
                        gathered_data_for_gemini.insert(
                            item_path_or_qname.clone(),
                            format!(
                                "// GEM_NOTE: Definition for {} not found.",
                                item_path_or_qname
                            ),
                        );
                    }
                    Err(e) => {
                        eprintln!(
                            "gem: ERROR: Failed to query for {}: {}",
                            item_path_or_qname, e
                        );
                        gathered_data_for_gemini.insert(
                            item_path_or_qname.clone(),
                            format!(
                                "// GEM_NOTE: Error querying for {}: {}",
                                item_path_or_qname, e
                            ),
                        );
                    }
                }
            }
        }
        current_needed_items.clear();

        let sufficiency_prompt =
            construct_sufficiency_check_prompt(&args.user_request, &gathered_data_for_gemini);

        if args.debug_mode == Some(crate::cli::DebugMode::Sufficient) {
            println!(
                "\n--- DEBUG: SUFFICIENCY PROMPT (Iteration {}) ---",
                data_gathering_iterations
            );
            println!("{}", sufficiency_prompt);
            println!("--- END DEBUG: SUFFICIENCY PROMPT ---");
            return Ok(());
        }

        println!("gem: Asking Gemini if gathered data is sufficient...");

        // NOTE: This still uses the mock.
        let gemini_sufficiency_response_str = clean_gemini_api_json(call_gemini_api_mock(
            &mut verification_attempt,
            &mut code_change_attempt,
            &gemini_api_key,
            &sufficiency_prompt,
            "GeminiSufficiencyResponse", // Mock needs this hint
                                         // FLASH_MODEL_NAME, // For real API call
        )?);
        let sufficiency_response: GeminiSufficiencyResponse =
            serde_json::from_str(&gemini_sufficiency_response_str)?;

        if sufficiency_response.sufficient {
            break;
        } else {
            if !sufficiency_response.needed_items.is_empty() {
                current_needed_items.extend(
                    sufficiency_response
                        .needed_items
                        .into_iter()
                        .filter(|item| !gathered_data_for_gemini.contains_key(item)),
                );
                if current_needed_items.is_empty() && !gathered_data_for_gemini.is_empty() {
                    println!("gem: All newly requested items were already gathered. Proceeding to check sufficiency again.");
                }
            } else {
                eprintln!("gem: ERROR: Gemini reported data is not sufficient but did not specify what's needed. Giving up.");
                return Err("Gemini insufficient without item list.".into());
            }
        }
    }

    // --- 4, 5, Git, 6. Code Generation, Application, Commit, and Verification Loop ---
    let mut verification_failures_context = String::new();

    loop {
        if verification_attempt >= args.max_verify_retries + 1 {
            eprintln!(
                "gem: ERROR: Exceeded maximum verification retries ({}). Giving up.",
                args.max_verify_retries
            );
            eprintln!("gem: The last unverified changes might be committed. Please review your git history.");
            return Err("Max verification retries reached.".into());
        }
        verification_attempt += 1;
        println!(
            "\ngem: Phase 3: Code Generation & Verification Attempt {}/{}...",
            verification_attempt,
            args.max_verify_retries + 1
        );

        let code_gen_prompt = construct_code_generation_prompt(
            &args.user_request,
            &gathered_data_for_gemini,
            !args.no_test,
            if verification_attempt > 1 {
                Some(&verification_failures_context)
            } else {
                None
            },
            &args.verify_with,
        );

        if args.debug_mode == Some(crate::cli::DebugMode::Changes) && verification_attempt == 1 {
            println!("\n--- DEBUG: CODE GENERATION PROMPT (Attempt 1) ---");
            println!("{}", code_gen_prompt);
            println!("--- END DEBUG: CODE GENERATION PROMPT ---");
            return Ok(());
        }

        println!("gem: Asking Gemini to generate code changes...");
        let gemini_code_gen_response_str = call_gemini_api_mock(
            &mut verification_attempt,
            &mut code_change_attempt,
            &gemini_api_key,
            &code_gen_prompt,
            "GeminiCodeGenerationResponse", // Mock needs this hint
                                            // THINKING_MODEL_NAME, // For real API call
        )?;
        let code_gen_response: GeminiCodeGenerationResponse =
            serde_json::from_str(&gemini_code_gen_response_str)?;

        println!(
            "gem: Gemini proposes changes: {}",
            code_gen_response.explanation
        );

        apply_code_changes_mock(&project_root, &code_gen_response.changes)?;

        if !args.no_test {
            if let Some(tests) = &code_gen_response.tests {
                apply_tests_mock(&project_root, tests)?;
            }
        }

        println!("gem: Verifying changes with: \"{}\"", args.verify_with);
        match execute_verification_command_mock(
            &mut verification_attempt,
            &project_root,
            &args.verify_with,
        ) {
            Ok(output) => {
                println!("gem: Verification successful!");
                println!("gem: Output:\n{}", output);

                let commit_message = format!(
                    "gem: Automated change for \"{}\"\n\n{}\n\n",
                    args.user_request, code_gen_response.explanation
                );
                git_commit_mock(&project_root, &commit_message, verification_attempt > 1)?;
                println!("\ngem: Task completed successfully.");

                return Ok(());
            }
            Err(e) => {
                println!("gem: Verification failed.");
                verification_failures_context = e.to_string();
                eprintln!("Error Output:\n{}", verification_failures_context);

                if verification_attempt >= args.max_verify_retries + 1 {
                    eprintln!("gem: Max verification retries reached. The last (failed) attempt is committed (amended). Please review and fix manually.");
                    return Err(format!(
                        "Verification failed after max retries: {}",
                        verification_failures_context
                    )
                    .into());
                }
                println!("gem: Will attempt to ask Gemini to fix the issues...");
            }
        }
    }
}

// --- Helper Functions (Implementations & Mocks) ---

fn check_dependencies(_project_root: &Path) -> Result<()> {
    // project_root not used here, but kept for consistency
    println!("gem: Checking dependencies (cargo, rustc, rust-analyzer)...");
    let deps = ["cargo", "rustc", "rust-analyzer"];
    for dep in deps.iter() {
        let dep_cmd = if cfg!(windows) && *dep == "rust-analyzer" {
            "rust-analyzer.exe"
        } else {
            dep
        };
        match Command::new(dep_cmd).arg("--version").output() {
            Ok(output) if output.status.success() => {
                // println!("gem: {} found.", dep);
            }
            Ok(output) => {
                let err_msg = format!(
                    "Dependency '{}' found but '--version' failed. Stderr: {}",
                    dep,
                    String::from_utf8_lossy(&output.stderr)
                );
                return Err(err_msg.into());
            }
            Err(e) => {
                let err_msg = format!(
                    "Dependency '{}' not found or not executable: {}. Please ensure it's installed and in PATH.",
                    dep, e
                );
                return Err(err_msg.into());
            }
        }
    }
    println!("gem: All dependencies found.");
    Ok(())
}

fn gather_initial_project_info(project_root: &Path) -> Result<HashMap<String, String>> {
    let mut context = HashMap::new();

    // --- Gather System Info ---
    let sysinfo = systeminfo::from_system_hardware();
    let osinfo = systeminfo::from_system_os();

    // Add relevant fields from system info to the context map
    context.insert(
        "system_manufacturer".to_string(),
        sysinfo.system_manufacturer,
    );
    context.insert("system_model".to_string(), sysinfo.system_model);
    context.insert("physical_memory".to_string(), sysinfo.physical_memory);
    context.insert("processor".to_string(), sysinfo.processor);
    context.insert("processor_vendor".to_string(), sysinfo.processor_vendor);
    context.insert(
        "processor_physical_cpus".to_string(),
        sysinfo.processor_physical_cpus,
    );
    context.insert(
        "processor_logical_cpus".to_string(),
        sysinfo.processor_logical_cpus,
    );

    // Join processor features (Vec<String>) into a single string
    context.insert(
        "processor_features".to_string(),
        sysinfo.processor_features.join(", "),
    );

    // Add relevant OS info fields
    // Using osinfo.os instead of env::consts::OS for more runtime-specific info
    context.insert("os".to_string(), osinfo.os);
    context.insert("kernel".to_string(), osinfo.kernel);
    context.insert("os_edition".to_string(), osinfo.edition);
    context.insert("os_version".to_string(), osinfo.version);

    // Assuming both system/processor and OS architecture are desired, use distinct keys
    context.insert("processor_architecture".to_string(), sysinfo.architecture);
    context.insert("os_architecture".to_string(), osinfo.architecture);

    // --- Gather Toolchain Versions (assuming get_command_output exists) ---
    context.insert(
        "rustc_version".to_string(),
        get_command_output("rustc", &["--version"])?,
    );
    context.insert(
        "cargo_version".to_string(),
        get_command_output("cargo", &["--version"])?,
    );
    context.insert(
        "rust_analyzer_version".to_string(),
        get_command_output(
            if cfg!(windows) {
                "rust-analyzer.exe"
            } else {
                "rust-analyzer"
            },
            &["--version"],
        )?,
    );

    println!("gem: Gathering project symbols...");
    match crate::parser::get_project_symbols_string(project_root) {
        Ok(symbols) => {
            context.insert("project_symbols".to_string(), symbols);
        }
        Err(e) => {
            eprintln!("gem: WARN: Failed to parse project symbols: {}", e);
            context.insert(
                "project_symbols".to_string(),
                "// Failed to parse project symbols".to_string(),
            );
        }
    }

    // --- Gather Project Structure and Dependencies (assuming helpers exist) ---

    // Ensure the src directory exists before generating the tree, or handle the error in generate_src_tree
    let src_dir = project_root.join("src");
    if src_dir.exists() && src_dir.is_dir() {
        context.insert("src_tree".to_string(), generate_src_tree(&src_dir)?);
    } else {
        context.insert(
            "src_tree".to_string(),
            "// No src directory found".to_string(),
        );
    }

    context.insert(
        "dependencies".to_string(),
        get_cargo_metadata_dependencies(project_root)?,
    );

    Ok(context)
}

fn get_command_output(cmd: &str, args: &[&str]) -> Result<String> {
    let output = Command::new(cmd).args(args).output()?;
    if output.status.success() {
        Ok(String::from_utf8(output.stdout)?.trim().to_string())
    } else {
        Err(format!(
            "Command `{} {:?}` failed: {}",
            cmd,
            args,
            String::from_utf8_lossy(&output.stderr)
        )
        .into())
    }
}

fn generate_src_tree(src_dir_path: &Path) -> Result<String> {
    if !src_dir_path.exists() || !src_dir_path.is_dir() {
        return Ok(format!(
            "Directory {} not found or is not a directory.",
            src_dir_path.display()
        ));
    }

    let mut tree_output = String::new();
    let walker = WalkBuilder::new(src_dir_path)
        .hidden(false) // Typically don't want to ignore hidden files like .DS_Store unless also in .gitignore
        .parents(true)
        .git_ignore(true)
        .git_global(true)
        .build();

    for result in walker {
        match result {
            Ok(entry) => {
                let path = entry.path();
                // Skip the root src_dir_path itself from the listing if depth is 0
                if entry.depth() == 0 && path == src_dir_path {
                    continue;
                }
                // Make path relative to src_dir_path for cleaner output
                let display_path = path.strip_prefix(src_dir_path).unwrap_or(path);
                let indent_level = if entry.depth() > 0 {
                    entry.depth() - 1
                } else {
                    0
                }; // Adjust indent
                let indent = "  ".repeat(indent_level);

                let entry_type_char = if entry.file_type().map_or(false, |ft| ft.is_dir()) {
                    "/"
                } else {
                    ""
                };

                tree_output.push_str(&format!(
                    "{}{}{}\n",
                    indent,
                    display_path.display(),
                    entry_type_char
                ));
            }
            Err(err) => eprintln!(
                "gem: WARN: error walking directory {} for src_tree: {}",
                src_dir_path.display(),
                err
            ),
        }
    }
    Ok(if tree_output.is_empty() {
        format!(
            "Directory {} is empty or all files are ignored.",
            src_dir_path.display()
        )
    } else {
        tree_output
    })
}

fn get_cargo_metadata_dependencies(project_root: &Path) -> Result<String> {
    let output = Command::new("cargo")
        .current_dir(project_root)
        .arg("tree")
        .output()?;

    if !output.status.success() {
        return Err(format!(
            "cargo metadata failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }

    Ok(String::from_utf8(output.stdout)?)
}

fn construct_first_gemini_prompt(user_request: &str, context: &HashMap<String, String>) -> String {
    format!(
        include_str!("prompts/initial.txt"),
        user_request,
        context.get("rustc_version").unwrap_or(&"N/A".to_string()),
        context.get("cargo_version").unwrap_or(&"N/A".to_string()),
        context
            .get("rust_analyzer_version")
            .unwrap_or(&"N/A".to_string()),
        context.get("os").unwrap_or(&"N/A".to_string()),
        context.get("src_tree").unwrap_or(&"N/A".to_string()),
        context.get("dependencies").unwrap_or(&"{}".to_string()),
        context.get("project_symbols").unwrap_or(&"N/A".to_string())
    )
}

fn construct_sufficiency_check_prompt(
    user_request: &str,
    gathered_data: &HashMap<String, String>,
) -> String {
    let mut data_str = String::new();
    for (item, content) in gathered_data {
        data_str.push_str(&format!(
            "// Item: {}\n// Extracted Code:\n{}\n\n",
            item, content
        ));
    }
    format!(
        include_str!("prompts/sufficient.txt"),
        user_request, data_str
    )
}

fn construct_code_generation_prompt(
    user_request: &str,
    gathered_data: &HashMap<String, String>,
    generate_tests: bool,
    failure_context: Option<&str>,
    verify_command: &str,
) -> String {
    let mut data_str = String::new();
    for (item, content) in gathered_data {
        data_str.push_str(&format!(
            "// Item: {}\n// Extracted Code:\n{}\n\n",
            item, content
        ));
    }

    let test_instruction = if generate_tests {
        "You should also generate relevant unit tests for the changes."
    } else {
        "Test generation is disabled for this request."
    };

    let failure_prompt_addition = if let Some(ctx) = failure_context {
        format!(
            r#"
Previous Attempt Feedback:
The verification command "{}" failed.
Build/Test Output (JSON messages or raw output):
```
{}
```
Please analyze the errors and provide a corrected set of changes and tests.
"#,
            verify_command, ctx
        )
    } else {
        String::new()
    };

    format!(
        include_str!("prompts/change.txt"),
        test_instruction, user_request, data_str, failure_prompt_addition, test_instruction
    )
}

fn call_real_gemini_api(api_key: &str, prompt_text: &str, model_name: &str) -> Result<String> {
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
        model_name, api_key
    );

    let request_payload = GeminiRequest {
        contents: vec![GeminiRequestContent {
            parts: vec![GeminiRequestPart {
                text: prompt_text.to_string(),
            }],
            role: Some("user".to_string()),
        }],
    };

    let client = reqwest::blocking::Client::new();
    let response = client
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&request_payload)
        .send()?;

    if response.status().is_success() {
        let response_body_text = response.text()?;
        let gemini_response: GeminiResponse = serde_json::from_str(&response_body_text)?;

        if let Some(candidate) = gemini_response.candidates.first() {
            if let Some(part) = candidate.content.parts.first() {
                Ok(part.text.clone()) // Return the actual text content from Gemini
            } else {
                Err("Gemini response missing content part".into())
            }
        } else {
            Err("Gemini response missing candidates".into())
        }
    } else {
        let status = response.status();
        let error_body = response
            .text()
            .unwrap_or_else(|_| "Could not read error body".to_string());
        Err(format!("Gemini API Error ({}): {}", status, error_body).into())
    }
}

// --- Mock function (kept for now, main uses this) ---
fn call_gemini_api_mock(
    retry_count: &mut usize,
    codegen_attempt: &mut usize,
    _api_key: &str,
    prompt: &str,
    expected_response_type: &str,
) -> Result<String> {
    // api_key not used in mock, but kept for signature compatibility if we toggle
    println!();

    match expected_response_type {
        "GeminiNeededItemsResponse" => {
            let main_lib_path = if Path::new("src/main.rs").exists() {
                "src/main.rs"
            } else {
                "src/lib.rs"
            };
            std::thread::sleep(std::time::Duration::from_secs(5));
            Ok(format!(
                r#"{{"needed_items": ["{}", "my_crate::some_module::SomeStruct"]}}"#,
                main_lib_path
            ))
        }
        "GeminiSufficiencyResponse" => {
            *retry_count += 1;
            std::thread::sleep(std::time::Duration::from_secs(5));
            if *retry_count > 1 || prompt.contains("SomeStruct") {
                let s = serde_json::to_string(&GeminiSufficiencyResponse {
                    sufficient: true,
                    needed_items: vec![],
                })
                .unwrap_or_default();

                Ok(s)
            } else {
                let s = serde_json::to_string(&GeminiSufficiencyResponse {
                    sufficient: false,
                    needed_items: vec!["my_crate::some_module::SomeOtherType".to_string()],
                })
                .unwrap_or_default();
                Ok(s)
            }
        }
        "GeminiCodeGenerationResponse" => {
            let explanation: String;
            let changes: Vec<CodeChange>;

            *codegen_attempt += 1;

            if prompt.contains("Previous Attempt Feedback") || *codegen_attempt > 1 {
                explanation =
                    "Fixed the previous build error by adding a missing import.".to_string();
                changes = vec![CodeChange {
                    file_path: "src/lib.rs".to_string(),
                    action: CodeChangeAction::ReplaceContent,
                    content: Some("use std::collections::HashMap;\n\npub fn hello() -> String { \"hello fixed\".to_string() }".to_string()),
                }];
            } else {
                explanation = "Initial attempt to refactor. Added a hello function.".to_string();
                changes = vec![CodeChange {
                    file_path: "src/lib.rs".to_string(),
                    action: CodeChangeAction::ReplaceContent,
                    content: Some("pub fn hello() -> String { \"hello initial\".to_string() } // This will fail verification".to_string()),
                }];
            }

            let tests = if !prompt.contains("Test generation is disabled") {
                Some(vec![TestChange {
                    file_path: "src/lib.rs".to_string(),
                    action: "append_to_file".to_string(),
                    content: "\n\n#[cfg(test)]\nmod tests {\n    use super::*;\n    #[test]\n    fn it_works() {\n        assert_eq!(hello(), \"hello fixed\");\n    }\n}".to_string(),
                    test_name: Some("it_works".to_string()),
                }])
            } else {
                None
            };

            std::thread::sleep(std::time::Duration::from_secs(5));

            let response = GeminiCodeGenerationResponse {
                changes,
                tests,
                explanation,
            };

            Ok(serde_json::to_string_pretty(&response)?)
        }
        _ => Err(format!(
            "Unknown expected_response_type for mock: {}",
            expected_response_type
        )
        .into()),
    }
}

// Gemini likes to output "markdown" format, even if specifically instructed not to do it.
fn clean_gemini_api_json(s: String) -> String {
    // TODO: use a real markdown parser and extract the first "json" block

    let s = if s.trim().starts_with("```json") {
        s.replacen("```json", "", 1)
    } else {
        s
    };

    let mut t = &s[..];
    if s.ends_with("```") {
        t = &s[0..(s.len() - 3)];
    }

    t.trim().to_string()
}

fn query_rust_analyzer_for_item_definition(
    project_root: &Path,
    item_qname_or_path: &str,
) -> Result<Option<String>> {
    println!("gem: Querying for item: {}", item_qname_or_path);

    // For file paths, just read the file
    if item_qname_or_path.ends_with(".rs") || item_qname_or_path.starts_with("src/") {
        let file_path = project_root.join(item_qname_or_path);
        if file_path.exists() {
            let content = fs::read_to_string(&file_path)?;
            return Ok(Some(content));
        } else {
            return Ok(None);
        }
    }

    // For qualified names, parse all files and search for the item
    let symbols = match parser::parse_directory(project_root) {
        Ok(s) => s,
        Err(e) => return Err(format!("Failed to parse project: {}", e).into()),
    };

    // Find the item in the symbols map
    if let Some(info) = symbols.get(item_qname_or_path) {
        // Return the hover text with file path info
        let file_path = Path::new(&info.source_vertex_id);
        let content = if file_path.exists() {
            format!("// From file: {:?}\n{}", file_path, info.hover_text)
        } else {
            info.hover_text.clone()
        };

        Ok(Some(content))
    } else {
        // Try partial matches (e.g., looking for struct fields)
        let matches: Vec<_> = symbols
            .iter()
            .filter(|(k, _)| k.contains(item_qname_or_path))
            .collect();

        if !matches.is_empty() {
            let mut content = String::from("// Multiple matches found:\n");
            for (key, info) in matches {
                content.push_str(&format!("// - {}: {:?}\n", key, info.symbol_type));
            }
            Ok(Some(content))
        } else {
            Ok(None)
        }
    }
}

fn apply_code_changes_mock(project_root: &Path, changes: &[CodeChange]) -> Result<()> {
    for change in changes {
        let target_path = project_root.join(&change.file_path);
        println!("gem: {:?} on file: {:?}", change.action, target_path);
        match change.action {
            CodeChangeAction::CreateFile | CodeChangeAction::ReplaceContent => {
                if let Some(content) = &change.content {
                    if let Some(parent) = target_path.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    let mut file = fs::File::create(&target_path)?;
                    file.write_all(content.as_bytes())?;
                } else {
                    eprintln!(
                        "  WARN: No content for CreateFile/ReplaceContent: {:?}",
                        target_path
                    );
                }
            }
            CodeChangeAction::DeleteFile => {
                if target_path.exists() {
                    fs::remove_file(&target_path)?;
                } else {
                    eprintln!("  WARN: File to delete not found: {:?}", target_path);
                }
            }
            _ => {
                println!(
                    "  Skipping mock application for action: {:?}",
                    change.action
                );
            }
        }
    }
    Ok(())
}

fn apply_tests_mock(project_root: &Path, tests: &[TestChange]) -> Result<()> {
    for test in tests {
        let target_path = project_root.join(&test.file_path);
        println!(
            "gem: {} on file: {:?}, Test: {:?}",
            test.action,
            target_path,
            test.test_name.as_deref().unwrap_or("N/A")
        );
        match test.action.as_str() {
            "append_to_file" => {
                if let Some(parent) = target_path.parent() {
                    fs::create_dir_all(parent)?;
                }
                let mut file = fs::OpenOptions::new()
                    .append(true)
                    .create(true)
                    .open(&target_path)?;
                file.write_all(test.content.as_bytes())?;
            }
            "create_file" => {
                if let Some(parent) = target_path.parent() {
                    fs::create_dir_all(parent)?;
                }
                let mut file = fs::File::create(&target_path)?;
                file.write_all(test.content.as_bytes())?;
            }
            _ => {
                println!(
                    "  Skipping mock application for test action: {}",
                    test.action
                );
            }
        }
    }
    Ok(())
}

fn git_commit_mock(project_root: &Path, message: &str, amend: bool) -> Result<()> {
    println!("\ngem: Phase 5: git commit");
    let amend = if amend { "--amend " } else { "" };
    println!("gem: git commit {amend}--message \"{message}\"");
    Ok(())
}

fn execute_verification_command_mock(
    attempt: &mut usize,
    project_root: &Path,
    command_str: &str,
) -> Result<String> {
    println!("\ngem: Phase 4: Verifying implementation with {command_str}");
    *attempt += 1;
    if *attempt == 1 && command_str.contains("cargo") {
        let err_output = r#"[{"reason": "compiler-message", ..., "success": false}]"#; // Simplified
        eprintln!("Mock: Verification FAILED (attempt {})", *attempt);
        Err(err_output.to_string().into())
    } else {
        println!("Mock: Verification SUCCESS (attempt {})", *attempt);
        Ok("Build successful!\nAll checks passed.".to_string())
    }
}

use gem::llm_api::{MockLLMApi, LLMApi, GeminiNeededItemsResponse, GeminiSufficiencyResponse, GeminiCodeGenerationResponse, CodeChange, CodeChangeAction}; // Added back some for new tests
use gem::cache::Session;
use gem::cli::CustomCliArgs;
use gem::run_gem_agent;
// Removed unused: construct_first_gemini_prompt, construct_sufficiency_check_prompt
// Removed unused: get_command_output, generate_src_tree, get_cargo_metadata_dependencies

// use std::collections::HashMap; // Currently not used by active tests
use std::path::PathBuf;
use tempfile::{tempdir, TempDir};
use std::fs;
use std::error::Error;

// Helper function to setup a test environment
// Returns project_root, a TempDir guard for project_root, and a TempDir guard for home_path
fn setup_test_env(session_id_prefix: &str) -> (PathBuf, TempDir, TempDir) {
    let temp_project_dir_guard = tempdir().unwrap(); // Guard for project root
    let project_root = temp_project_dir_guard.path().to_path_buf();

    let src_dir = project_root.join("src");
    fs::create_dir_all(&src_dir).unwrap();
    fs::write(src_dir.join("lib.rs"), "pub fn hello() {} \n pub struct SomeStruct;").unwrap();
    fs::write(project_root.join("Cargo.toml"), "[package]\nname = \"test_project\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\n").unwrap();

    // Create a unique session_id for each test run to avoid conflicts if HOME is shared or not perfectly isolated
    let unique_session_id = format!("{}_{}", session_id_prefix, uuid::Uuid::new_v4());

    // Mock HOME directory for session storage
    let temp_home_dir = tempdir().unwrap();
    let home_path_str = temp_home_dir.path().to_str().unwrap().to_string();
    std::env::set_var("HOME", &home_path_str);

    // Create the .gem/session/<session_id> structure within the mocked HOME
    let session_dir = PathBuf::from(&home_path_str).join(".gem").join("session").join(unique_session_id);
    fs::create_dir_all(&session_dir).unwrap();

    // The TempDir for project_root needs to be kept alive by the caller.
    (project_root, temp_project_dir_guard, temp_home_dir)
}

// Helper function to run the core logic with a mock API
fn run_gem_logic_with_mock_api_owned( // Renamed to avoid conflict if there was a previous version
    args: CustomCliArgs,
    mock_api: MockLLMApi, // Takes ownership as it's usually configured per test
    project_root: PathBuf,
) -> Result<Session, Box<dyn Error>> {

    // Ensure a unique session_id for this run based on potentially varying args
    let session_id_str = format!("{:?}_{:?}", args, std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_nanos());
    let session_id = Session::compute_hash(&session_id_str);
    let mut session = Session::new(&session_id); // Session uses HOME env var set in setup_test_env

    let llm_api_boxed: Box<dyn LLMApi> = Box::new(mock_api);

    match run_gem_agent(args, &mut session, llm_api_boxed, false, project_root) {
        Ok(_) => Ok(session),
        Err(e) => {
            // Optionally print the error for easier debugging in tests
            // eprintln!("run_gem_logic_with_mock_api error: {:?}", e);
            Err(e)
        }
    }
}

/*
#[test]
fn test_initial_prompt_flow() -> Result<(), Box<dyn Error>> {
    let (project_root, _project_dir_guard, _home_dir_guard) = setup_test_env("initial_prompt");

    let mut args = CustomCliArgs::default();
    args.user_request = "test request for initial items".to_string();
    args.project_root = project_root.clone();
    // Keep other args as default for this test

    let mut mock_api = MockLLMApi::new();
    let expected_needed_items = vec!["src/lib.rs".to_string(), "test_project::SomeStruct".to_string()];

    // Construct the expected initial prompt to use as the key for the mock response
    // Gather minimal context similar to what gather_initial_project_info would do.
    // This is a simplification; real context gathering is more complex.
    let mut dummy_context = HashMap::new();
    dummy_context.insert("rustc_version".to_string(), "mock_rustc_version".to_string());
    dummy_context.insert("cargo_version".to_string(), "mock_cargo_version".to_string());
    dummy_context.insert("rust_analyzer_version".to_string(), "mock_ra_version".to_string());
    dummy_context.insert("os".to_string(), "mock_os".to_string());
    dummy_context.insert("src_tree".to_string(), "src/\nsrc/lib.rs\n".to_string()); // Simplified
    dummy_context.insert("dependencies".to_string(), "test_project v0.1.0\n".to_string()); // Simplified
    dummy_context.insert("project_symbols".to_string(), "// pub fn hello()\n// pub struct SomeStruct\n".to_string()); // Simplified

    let first_prompt_text = construct_first_gemini_prompt(&args.user_request, &dummy_context);

    let response_json = serde_json::to_string(&GeminiNeededItemsResponse {
        needed_items: expected_needed_items.clone(),
    })?;
    mock_api.add_mock_response(&first_prompt_text, Ok(response_json));

    // Mock for the sufficiency check that will follow (assuming it becomes sufficient quickly)
    // This prompt text is also dynamic. For a focused test on initial_prompt_flow,
    // we might not even reach sufficiency check if we could inspect `current_needed_items` earlier.
    // For now, let's assume it asks for sufficiency with the items it got.
    let mut gathered_for_sufficiency = HashMap::new();
    gathered_for_sufficiency.insert("src/lib.rs".to_string(), "pub fn hello() {} \n pub struct SomeStruct;".to_string());
    gathered_for_sufficiency.insert("test_project::SomeStruct".to_string(), "pub struct SomeStruct;".to_string());

    // The actual sufficiency prompt will be more complex. This is a placeholder.
    // This highlights the difficulty of mocking exact prompt strings.
    let sufficiency_prompt_text = gem::construct_sufficiency_check_prompt(&args.user_request, &gathered_for_sufficiency);
    let sufficient_response = GeminiSufficiencyResponse { sufficient: true, needed_items: vec![] };
    mock_api.add_mock_response(&sufficiency_prompt_text, Ok(serde_json::to_string(&sufficient_response)?));

    let result_session = run_gem_logic_with_mock_api_owned(args, mock_api, project_root)?;

    // Assertions:
    // The `current_needed_items` is internal to `run_gem_agent`.
    // We can check `session.gathered_data` because items are added there.
    // Note: query_rust_analyzer_for_item_definition is mocked in lib.rs to return item name for now.
    // So, the content in gathered_data will be the item name itself.
    for item in expected_needed_items {
        assert!(result_session.gathered_data.contains_key(&item), "Expected item '{}' not found in gathered_data", item);
        // A more robust check would be if the *actual content* of src/lib.rs was fetched for "src/lib.rs"
        // and if "test_project::SomeStruct" resulted in "pub struct SomeStruct;"
        // This depends on query_rust_analyzer_for_item_definition behavior.
        // The test setup writes "pub fn hello() {} \n pub struct SomeStruct;" to "src/lib.rs".
        // The locatesource module should pick this up.
    }
    // Example of checking content for src/lib.rs:
    assert_eq!(result_session.gathered_data.get("src/lib.rs").unwrap(), "pub fn hello() {} \n pub struct SomeStruct;");
    // For "test_project::SomeStruct", locatesource will try to find its definition.
    // If locatesource::retrieve_item_source is robust, it should find "pub struct SomeStruct;"
    // For this test, we assume locatesource works. The key is that the LLM asked for it.

    Ok(())
}

#[test]
fn test_code_generation_flow_applies_changes() -> Result<(), Box<dyn Error>> {
    let (project_root, _project_dir_guard, _home_dir_guard) = setup_test_env("code_gen_applies_changes");

    let mut args = CustomCliArgs::default();
    args.user_request = "create a new file".to_string();
    args.project_root = project_root.clone();

    let mut mock_api = MockLLMApi::new();

    // 1. Initial prompt response
    let initial_response = GeminiNeededItemsResponse { needed_items: vec![] }; // Assume no items needed for simplicity
    mock_api.add_mock_response(Ok(serde_json::to_string(&initial_response)?));

    // 2. Sufficiency prompt response (directly sufficient)
    let sufficiency_response = GeminiSufficiencyResponse { sufficient: true, needed_items: vec![] };
    mock_api.add_mock_response(Ok(serde_json::to_string(&sufficiency_response)?));

    // 3. Change prompt response (the actual change)
    let new_file_path = "src/new_file_from_test.txt";
    let new_file_content = "hello from test";
    let change = gem::llm_api::CodeChange {
        file_path: new_file_path.to_string(),
        action: gem::llm_api::CodeChangeAction::CreateFile,
        content: Some(new_file_content.to_string()),
    };
    let code_gen_response = gem::llm_api::GeminiCodeGenerationResponse {
        changes: vec![change],
        tests: None,
        explanation: "Test creating a file".to_string(),
    };
    mock_api.add_mock_response(Ok(serde_json::to_string(&code_gen_response)?));

    // Run the agent logic
    let result = run_gem_logic_with_mock_api_owned(args, mock_api, project_root.clone());
    assert!(result.is_ok(), "run_gem_logic_with_mock_api_owned failed: {:?}", result.err());

    // Assert file creation and content
    let expected_file = project_root.join(new_file_path);
    assert!(expected_file.exists(), "Expected file {:?} was not created", expected_file);
    assert_eq!(fs::read_to_string(expected_file)?, new_file_content);

    Ok(())
}
*/

// TODO: Add more tests:
// test_sufficiency_loop_sufficient_case
// test_sufficiency_loop_insufficient_case
// test_code_generation_flow
// test_api_error_handling (e.g. MockLLMApi returns Err)

#[test]
fn test_sufficiency_loop_sufficient_case() -> Result<(), Box<dyn Error>> {
    let (project_root, _project_dir_guard, _home_dir_guard) = setup_test_env("sufficiency_sufficient");

    let mut args = CustomCliArgs::default();
    args.user_request = "test sufficiency: sufficient case".to_string();
    args.project_root = project_root.clone();

    let mut mock_api = MockLLMApi::new();

    // 1. Initial prompt response
    let initial_response = GeminiNeededItemsResponse { needed_items: vec!["src/lib.rs".to_string()] };
    mock_api.add_mock_response(Ok(serde_json::to_string(&initial_response)?));

    // 2. Sufficiency prompt response
    let sufficiency_response = GeminiSufficiencyResponse { sufficient: true, needed_items: vec![] };
    mock_api.add_mock_response(Ok(serde_json::to_string(&sufficiency_response)?));

    // 3. Change prompt response (agent should proceed to this)
    let change_response = gem::llm_api::GeminiCodeGenerationResponse { // Use full path
        changes: vec![], // No actual changes needed for this test's assertion
        tests: None,
        explanation: "Proceeded to code generation as data was sufficient.".to_string(),
    };
    mock_api.add_mock_response(Ok(serde_json::to_string(&change_response)?));

    // Run the agent logic
    let result = run_gem_logic_with_mock_api_owned(args, mock_api, project_root);

    // Assert that the entire flow completed without error (i.e., all mock responses were consumed)
    assert!(result.is_ok(), "run_gem_logic_with_mock_api_owned failed: {:?}", result.err());

    Ok(())
}
// test_max_loops_respected
// ... etc.

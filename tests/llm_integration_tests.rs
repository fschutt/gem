use gem::llm_api::{MockLLMApi, LLMApi, GeminiNeededItemsResponse, GeminiSufficiencyResponse, GeminiCodeGenerationResponse, CodeChange, CodeChangeAction};
use gem::cache::Session;
use gem::cli::{CustomCliArgs, MAX_DATA_GATHERING_ITERATIONS_DEFAULT, MAX_VERIFICATION_RETRIES_DEFAULT};
use gem::run_gem_agent;
use std::path::PathBuf;
use tempfile::{tempdir, TempDir};
use std::fs;
use std::error::Error;
use serial_test::serial;

fn common_test_args(project_root: PathBuf, user_request: &str) -> CustomCliArgs {
    CustomCliArgs {
        user_request_parts: vec![user_request.to_string()],
        project_root,
        verify_with: "cargo check".to_string(),
        no_test: false,
        project_file: None,
        max_data_loops: MAX_DATA_GATHERING_ITERATIONS_DEFAULT,
        max_verify_retries: MAX_VERIFICATION_RETRIES_DEFAULT,
        debug_mode: None,
        no_explanation: false,
        no_code: false,
        no_readme: false,
        auto_tool_selection: false,
        browser: None,
        input_selector: None,
        codeblock_selector: None,
        finished_selector: None,
        local: false,
    }
}

// Helper function to setup a test environment
// Returns project_root, a TempDir guard for project_root, and a TempDir guard for home_path
fn setup_test_env(session_id_prefix: &str) -> (PathBuf, TempDir, TempDir) {
    let temp_project_dir_guard = tempdir().unwrap(); // Guard for project root
    let project_root = temp_project_dir_guard.path().to_path_buf();

    let src_dir = project_root.join("src");
    fs::create_dir_all(&src_dir).unwrap();
    fs::write(src_dir.join("lib.rs"), "pub fn hello() {} \n pub struct SomeStruct;").unwrap();
    fs::write(project_root.join("Cargo.toml"), "[package]\nname = \"test_project\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\n").unwrap();

    let unique_session_id = format!("{}_{}", session_id_prefix, uuid::Uuid::new_v4());

    let temp_home_dir = tempdir().unwrap();
    let home_path_str = temp_home_dir.path().to_str().unwrap().to_string();
    std::env::set_var("HOME", &home_path_str);

    let session_dir = PathBuf::from(&home_path_str).join(".gem").join("session").join(unique_session_id);
    fs::create_dir_all(&session_dir).unwrap();

    (project_root, temp_project_dir_guard, temp_home_dir)
}

fn run_gem_logic_with_mock_api_owned(
    args: CustomCliArgs,
    mock_api: MockLLMApi,
    project_root: PathBuf,
) -> Result<Session, Box<dyn Error>> {
    let session_id_str = format!("{:?}_{:?}", args, std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_nanos());
    let session_id = Session::compute_hash(&session_id_str);
    let mut session = Session::new(&session_id);

    let llm_api_boxed: Box<dyn LLMApi> = Box::new(mock_api);

    match run_gem_agent(args, &mut session, llm_api_boxed, false, project_root) {
        Ok(_) => Ok(session),
        Err(e) => Err(e),
    }
}


#[test]
#[serial]
fn test_initial_prompt_flow() -> Result<(), Box<dyn Error>> {
    let (project_root, _project_dir_guard, _home_dir_guard) = setup_test_env("initial_prompt");

    let args = common_test_args(project_root.clone(), "test request for initial items");

    let mut mock_api = MockLLMApi::new();
    let expected_needed_items = vec!["src/lib.rs".to_string(), "test_project::SomeStruct".to_string()];

    let response_json = serde_json::to_string(&GeminiNeededItemsResponse {
        needed_items: expected_needed_items.clone(),
    })?;
    mock_api.add_mock_response(Ok(response_json));

    let sufficient_response = GeminiSufficiencyResponse { sufficient: true, needed_items: vec![] };
    mock_api.add_mock_response(Ok(serde_json::to_string(&sufficient_response)?));

    let code_gen_response = GeminiCodeGenerationResponse {
        changes: vec![],
        tests: None,
        explanation: "Mocked code generation response for initial_prompt_flow test.".to_string(),
    };
    mock_api.add_mock_response(Ok(serde_json::to_string(&code_gen_response)?));

    let result_session = run_gem_logic_with_mock_api_owned(args, mock_api, project_root)?;

    for item in expected_needed_items {
        assert!(result_session.gathered_data.contains_key(&item), "Expected item '{}' not found in gathered_data", item);
    }
    assert_eq!(result_session.gathered_data.get("src/lib.rs").unwrap(), "pub fn hello() {} \n pub struct SomeStruct;");
    Ok(())
}

#[test]
#[serial]
fn test_markdown_remove_function_expect_whole_file_replace() -> Result<(), Box<dyn Error>> {
    let (project_root, _project_dir_guard, _home_dir_guard) = setup_test_env("md_remove_func_specific");

    let initial_lib_content = r#"
// Initial file content
pub fn function_to_remove() -> i32 {
    100
}

pub fn function_to_keep() -> String {
    "keep_me".to_string()
}
"#;
    fs::write(project_root.join("src").join("lib.rs"), initial_lib_content)?;

    let args = common_test_args(project_root.clone(), "Remove function_to_remove via Markdown");

    let mut mock_api = MockLLMApi::new();
    mock_api.add_mock_response(Ok(serde_json::to_string(&GeminiNeededItemsResponse { needed_items: vec![] })?));
    mock_api.add_mock_response(Ok(serde_json::to_string(&GeminiSufficiencyResponse { sufficient: true, needed_items: vec![] })?));

    let markdown_content = r#"
Explanation: `function_to_remove` has been removed. The file now only contains `function_to_keep`.

File: src/lib.rs
```rust
// Initial file content

pub fn function_to_keep() -> String {
    "keep_me".to_string()
}
```
The function `function_to_remove` is gone.
"#;
    let expected_final_content = r#"
// Initial file content

pub fn function_to_keep() -> String {
    "keep_me".to_string()
}
"#.trim();

    let change = CodeChange {
        file_path: "MARKDOWN_CHANGES".to_string(),
        action: CodeChangeAction::ProcessMarkdownAndApplyChanges,
        content: Some(markdown_content.to_string()),
    };
    let llm_response = GeminiCodeGenerationResponse {
        changes: vec![change],
        tests: None,
        explanation: "High-level: Removed a function via Markdown.".to_string(),
    };
    mock_api.add_mock_response(Ok(serde_json::to_string(&llm_response)?));

    run_gem_logic_with_mock_api_owned(args, mock_api, project_root.clone())?;

    let modified_content = fs::read_to_string(project_root.join("src").join("lib.rs"))?;

    assert_eq!(modified_content.trim(), expected_final_content);
    assert!(!modified_content.contains("pub fn function_to_remove()"));
    assert!(modified_content.contains("pub fn function_to_keep()"));

    Ok(())
}

#[test]
#[serial]
fn test_markdown_add_new_function_expect_whole_file_replace() -> Result<(), Box<dyn Error>> {
    let (project_root, _project_dir_guard, _home_dir_guard) = setup_test_env("md_add_func_specific");

    let initial_lib_content = r#"
// Initial file content
pub fn existing_function() {}
"#;
    fs::write(project_root.join("src").join("lib.rs"), initial_lib_content)?;

    let args = common_test_args(project_root.clone(), "Add a new function new_function_to_add via Markdown");

    let mut mock_api = MockLLMApi::new();
    mock_api.add_mock_response(Ok(serde_json::to_string(&GeminiNeededItemsResponse { needed_items: vec![] })?));
    mock_api.add_mock_response(Ok(serde_json::to_string(&GeminiSufficiencyResponse { sufficient: true, needed_items: vec![] })?));

    let markdown_content = r#"
Adding a new function `new_function_to_add`.

File: src/lib.rs
```rust
pub fn new_function_to_add() -> bool {
    true
}
```
This function has been added.
"#;
    let expected_final_content = r#"
pub fn new_function_to_add() -> bool {
    true
}
"#.trim();

    let change = CodeChange {
        file_path: "MARKDOWN_CHANGES".to_string(),
        action: CodeChangeAction::ProcessMarkdownAndApplyChanges,
        content: Some(markdown_content.to_string()),
    };
    let llm_response = GeminiCodeGenerationResponse {
        changes: vec![change],
        tests: None,
        explanation: "High-level: Added a new function via Markdown.".to_string(),
    };
    mock_api.add_mock_response(Ok(serde_json::to_string(&llm_response)?));

    run_gem_logic_with_mock_api_owned(args, mock_api, project_root.clone())?;

    let modified_content = fs::read_to_string(project_root.join("src").join("lib.rs"))?;

    assert_eq!(modified_content.trim(), expected_final_content);
    assert!(!modified_content.contains("pub fn existing_function() {}"));

    Ok(())
}

#[test]
#[serial]
fn test_markdown_replace_existing_const() -> Result<(), Box<dyn Error>> {
    let (project_root, _project_dir_guard, _home_dir_guard) = setup_test_env("md_replace_const_specific");

    let initial_lib_content = r#"
// This is a constant that will be replaced.
pub const CONST_TO_REPLACE: i32 = 123;
// Another item to ensure it's not disturbed.
pub fn some_other_function() {}
"#;
    fs::write(project_root.join("src").join("lib.rs"), initial_lib_content)?;

    let args = common_test_args(project_root.clone(), "Replace CONST_TO_REPLACE via Markdown");

    let mut mock_api = MockLLMApi::new();
    mock_api.add_mock_response(Ok(serde_json::to_string(&GeminiNeededItemsResponse { needed_items: vec![] })?));
    mock_api.add_mock_response(Ok(serde_json::to_string(&GeminiSufficiencyResponse { sufficient: true, needed_items: vec![] })?));

    let markdown_content = r#"
The constant `CONST_TO_REPLACE` needs an update.

File: src/lib.rs
```rust
pub const CONST_TO_REPLACE: &str = "new_value";
```
The constant has been changed to a string type and new value.
"#;

    let change = CodeChange {
        file_path: "MARKDOWN_CHANGES".to_string(),
        action: CodeChangeAction::ProcessMarkdownAndApplyChanges,
        content: Some(markdown_content.to_string()),
    };
    let llm_response = GeminiCodeGenerationResponse {
        changes: vec![change],
        tests: None,
        explanation: "High-level: Updated a const via Markdown.".to_string(),
    };
    mock_api.add_mock_response(Ok(serde_json::to_string(&llm_response)?));

    run_gem_logic_with_mock_api_owned(args, mock_api, project_root.clone())?;

    let modified_content = fs::read_to_string(project_root.join("src").join("lib.rs"))?;

    assert!(modified_content.contains("pub const CONST_TO_REPLACE: &str = \"new_value\";"));
    assert!(!modified_content.contains("pub const CONST_TO_REPLACE: i32 = 123;"));
    assert!(modified_content.contains("pub fn some_other_function() {}"));
    assert!(modified_content.contains("// This is a constant that will be replaced."));

    Ok(())
}

#[test]
#[serial]
fn test_markdown_replace_existing_enum() -> Result<(), Box<dyn Error>> {
    let (project_root, _project_dir_guard, _home_dir_guard) = setup_test_env("md_replace_enum_specific");

    let initial_lib_content = r#"
pub enum EnumToReplace {
    VariantA,
    VariantB(i32),
}
const MY_CONST: bool = true; // Should remain
"#;
    fs::write(project_root.join("src").join("lib.rs"), initial_lib_content)?;

    let args = common_test_args(project_root.clone(), "Replace EnumToReplace via Markdown");

    let mut mock_api = MockLLMApi::new();
    mock_api.add_mock_response(Ok(serde_json::to_string(&GeminiNeededItemsResponse { needed_items: vec![] })?));
    mock_api.add_mock_response(Ok(serde_json::to_string(&GeminiSufficiencyResponse { sufficient: true, needed_items: vec![] })?));

    let markdown_content = r#"
This Markdown explains that `EnumToReplace` will be updated.

File: src/lib.rs
```rust
pub enum EnumToReplace {
    NewVariant,
    AnotherVariant { name: String },
}
```
The enum has been changed to have new variants.
"#;

    let change = CodeChange {
        file_path: "MARKDOWN_CHANGES".to_string(),
        action: CodeChangeAction::ProcessMarkdownAndApplyChanges,
        content: Some(markdown_content.to_string()),
    };
    let llm_response = GeminiCodeGenerationResponse {
        changes: vec![change],
        tests: None,
        explanation: "High-level: Updated an enum via Markdown.".to_string(),
    };
    mock_api.add_mock_response(Ok(serde_json::to_string(&llm_response)?));

    run_gem_logic_with_mock_api_owned(args, mock_api, project_root.clone())?;

    let modified_content = fs::read_to_string(project_root.join("src").join("lib.rs"))?;

    assert!(modified_content.contains("pub enum EnumToReplace {"));
    assert!(modified_content.contains("NewVariant,"));
    assert!(modified_content.contains("AnotherVariant { name: String },"));
    assert!(!modified_content.contains("VariantA,"));
    assert!(!modified_content.contains("VariantB(i32),"));
    assert!(modified_content.contains("const MY_CONST: bool = true;"));

    Ok(())
}

#[test]
#[serial]
fn test_markdown_replace_existing_struct() -> Result<(), Box<dyn Error>> {
    let (project_root, _project_dir_guard, _home_dir_guard) = setup_test_env("md_replace_struct_specific");

    let initial_lib_content = r#"
// Comment before struct
pub struct StructToReplace {
    old_field: bool,
}
// Comment after struct
fn another_item() {}
"#;
    fs::write(project_root.join("src").join("lib.rs"), initial_lib_content)?;

    let args = common_test_args(project_root.clone(), "Replace StructToReplace via Markdown");

    let mut mock_api = MockLLMApi::new();
    mock_api.add_mock_response(Ok(serde_json::to_string(&GeminiNeededItemsResponse { needed_items: vec![] })?));
    mock_api.add_mock_response(Ok(serde_json::to_string(&GeminiSufficiencyResponse { sufficient: true, needed_items: vec![] })?));

    let markdown_content = r#"
Replacing `StructToReplace`.

File: src/lib.rs
```rust
pub struct StructToReplace {
    new_field: String,
    count: i64,
}
```
The struct has been updated with new fields.
"#;

    let change = CodeChange {
        file_path: "MARKDOWN_CHANGES".to_string(),
        action: CodeChangeAction::ProcessMarkdownAndApplyChanges,
        content: Some(markdown_content.to_string()),
    };
    let llm_response = GeminiCodeGenerationResponse {
        changes: vec![change],
        tests: None,
        explanation: "High-level: Updated a struct via Markdown.".to_string(),
    };
    mock_api.add_mock_response(Ok(serde_json::to_string(&llm_response)?));

    run_gem_logic_with_mock_api_owned(args, mock_api, project_root.clone())?;

    let modified_content = fs::read_to_string(project_root.join("src").join("lib.rs"))?;

    assert!(modified_content.contains("pub struct StructToReplace {"));
    assert!(modified_content.contains("new_field: String,"));
    assert!(modified_content.contains("count: i64,"));
    assert!(!modified_content.contains("old_field: bool,"));
    assert!(modified_content.contains("fn another_item() {}"));
    assert!(modified_content.contains("// Comment before struct"));
    assert!(modified_content.contains("// Comment after struct"));

    Ok(())
}

#[test]
#[serial]
fn test_markdown_item_replacement_block_not_single_item_fallback() -> Result<(), Box<dyn Error>> {
    let (project_root, _project_dir_guard, _home_dir_guard) = setup_test_env("md_block_not_single_item");

    let initial_lib_content = r#"
pub fn old_func() {}
"#;
    fs::write(project_root.join("src").join("lib.rs"), initial_lib_content)?;

    let args = common_test_args(project_root.clone(), "Replace lib.rs with multiple items from markdown, expecting fallback");

    let mut mock_api = MockLLMApi::new();
    mock_api.add_mock_response(Ok(serde_json::to_string(&GeminiNeededItemsResponse { needed_items: vec![] })?));
    mock_api.add_mock_response(Ok(serde_json::to_string(&GeminiSufficiencyResponse { sufficient: true, needed_items: vec![] })?));

    let markdown_content = r#"
src/lib.rs
```rust
pub fn func_a() {}
pub struct StructB {}
// Not a single parsable syn::Item
```
"#;
    let new_block_content = r#"
pub fn func_a() {}
pub struct StructB {}
// Not a single parsable syn::Item
"#.trim();

    let change = CodeChange {
        file_path: "MARKDOWN_CHANGES".to_string(),
        action: CodeChangeAction::ProcessMarkdownAndApplyChanges,
        content: Some(markdown_content.to_string()),
    };
    let code_gen_response = GeminiCodeGenerationResponse {
        changes: vec![change],
        tests: None,
        explanation: "Markdown block not a single item, expecting fallback to whole file.".to_string(),
    };
    mock_api.add_mock_response(Ok(serde_json::to_string(&code_gen_response)?));

    let result = run_gem_logic_with_mock_api_owned(args, mock_api, project_root.clone());
    assert!(result.is_ok(), "run_gem_logic_with_mock_api_owned failed: {:?}", result.err());

    let modified_content = fs::read_to_string(project_root.join("src").join("lib.rs"))?;

    assert_eq!(modified_content.trim(), new_block_content);
    assert!(!modified_content.contains("old_func"));

    Ok(())
}


#[test]
#[serial]
fn test_markdown_item_replacement_target_file_does_not_exist_fallback() -> Result<(), Box<dyn Error>> {
    let (project_root, _project_dir_guard, _home_dir_guard) = setup_test_env("md_target_file_not_exist");

    let args = common_test_args(project_root.clone(), "Create new_file_for_item.rs with a function via markdown");

    let mut mock_api = MockLLMApi::new();
    mock_api.add_mock_response(Ok(serde_json::to_string(&GeminiNeededItemsResponse { needed_items: vec![] })?));
    mock_api.add_mock_response(Ok(serde_json::to_string(&GeminiSufficiencyResponse { sufficient: true, needed_items: vec![] })?));

    let markdown_content = r#"
src/new_file_for_item.rs
```rust
pub fn func_in_new_file() {}
```
"#;
    let new_function_code = "pub fn func_in_new_file() {}";

    let change = CodeChange {
        file_path: "MARKDOWN_CHANGES".to_string(),
        action: CodeChangeAction::ProcessMarkdownAndApplyChanges,
        content: Some(markdown_content.to_string()),
    };
    let code_gen_response = GeminiCodeGenerationResponse {
        changes: vec![change],
        tests: None,
        explanation: "Attempted item replacement in non-existent file, expecting fallback to create file.".to_string(),
    };
    mock_api.add_mock_response(Ok(serde_json::to_string(&code_gen_response)?));

    let result = run_gem_logic_with_mock_api_owned(args, mock_api, project_root.clone());
    assert!(result.is_ok(), "run_gem_logic_with_mock_api_owned failed: {:?}", result.err());

    let created_content = fs::read_to_string(project_root.join("src").join("new_file_for_item.rs"))?;

    assert_eq!(created_content.trim(), new_function_code.trim());

    Ok(())
}

#[test]
#[serial]
fn test_markdown_item_replacement_item_not_in_file_fallback() -> Result<(), Box<dyn Error>> {
    let (project_root, _project_dir_guard, _home_dir_guard) = setup_test_env("md_item_not_in_file");

    let initial_lib_content = r#"
pub fn existing_func() {}
"#;
    fs::write(project_root.join("src").join("lib.rs"), initial_lib_content)?;

    let args = common_test_args(project_root.clone(), "Add new_func_from_markdown to lib.rs, expecting fallback");

    let mut mock_api = MockLLMApi::new();
    mock_api.add_mock_response(Ok(serde_json::to_string(&GeminiNeededItemsResponse { needed_items: vec![] })?));
    mock_api.add_mock_response(Ok(serde_json::to_string(&GeminiSufficiencyResponse { sufficient: true, needed_items: vec![] })?));

    let markdown_content = r#"
src/lib.rs
```rust
pub fn new_func_from_markdown() { /* new function */ }
```
"#;
    let new_function_code = "pub fn new_func_from_markdown() { /* new function */ }";

    let change = CodeChange {
        file_path: "MARKDOWN_CHANGES".to_string(),
        action: CodeChangeAction::ProcessMarkdownAndApplyChanges,
        content: Some(markdown_content.to_string()),
    };
    let code_gen_response = GeminiCodeGenerationResponse {
        changes: vec![change],
        tests: None,
        explanation: "Attempted item replacement, expecting fallback to whole file.".to_string(),
    };
    mock_api.add_mock_response(Ok(serde_json::to_string(&code_gen_response)?));

    let result = run_gem_logic_with_mock_api_owned(args, mock_api, project_root.clone());
    assert!(result.is_ok(), "run_gem_logic_with_mock_api_owned failed: {:?}", result.err());

    let modified_content = fs::read_to_string(project_root.join("src").join("lib.rs"))?;

    assert_eq!(modified_content.trim(), new_function_code.trim());
    assert!(!modified_content.contains("existing_func"));

    Ok(())
}

#[test]
#[serial]
fn test_markdown_item_replacement_struct_exists() -> Result<(), Box<dyn Error>> {
    let (project_root, _project_dir_guard, _home_dir_guard) = setup_test_env("md_item_replace_struct");

    let initial_lib_content = r#"
pub struct StructToReplace { old_field: i32 }
pub fn some_func_after_struct() {} // Should remain
"#;
    fs::write(project_root.join("src").join("lib.rs"), initial_lib_content)?;

    let args = common_test_args(project_root.clone(), "Replace StructToReplace via markdown");

    let mut mock_api = MockLLMApi::new();
    mock_api.add_mock_response(Ok(serde_json::to_string(&GeminiNeededItemsResponse { needed_items: vec![] })?));
    mock_api.add_mock_response(Ok(serde_json::to_string(&GeminiSufficiencyResponse { sufficient: true, needed_items: vec![] })?));

    let markdown_content = r#"
src/lib.rs
```rust
pub struct StructToReplace { new_field: String, another_field: bool }
```
"#;
    let change = CodeChange {
        file_path: "MARKDOWN_CHANGES".to_string(),
        action: CodeChangeAction::ProcessMarkdownAndApplyChanges,
        content: Some(markdown_content.to_string()),
    };
    let code_gen_response = GeminiCodeGenerationResponse {
        changes: vec![change],
        tests: None,
        explanation: "Replaced item StructToReplace from Markdown.".to_string(),
    };
    mock_api.add_mock_response(Ok(serde_json::to_string(&code_gen_response)?));

    let result = run_gem_logic_with_mock_api_owned(args, mock_api, project_root.clone());
    assert!(result.is_ok(), "run_gem_logic_with_mock_api_owned failed: {:?}", result.err());

    let modified_content = fs::read_to_string(project_root.join("src").join("lib.rs"))?;

    assert!(modified_content.contains("pub struct StructToReplace { new_field: String, another_field: bool }"));
    assert!(modified_content.contains("pub fn some_func_after_struct() {}"));
    assert!(!modified_content.contains("old_field: i32"));

    Ok(())
}

#[test]
#[serial]
fn test_markdown_processing_filename_heuristics() -> Result<(), Box<dyn Error>> {
    let (project_root, _project_dir_guard, _home_dir_guard) = setup_test_env("md_filename_heuristics");

    let args = common_test_args(project_root.clone(), "Create files from markdown with various filename styles");

    let mut mock_api = MockLLMApi::new();
    mock_api.add_mock_response(Ok(serde_json::to_string(&GeminiNeededItemsResponse { needed_items: vec![] })?));
    mock_api.add_mock_response(Ok(serde_json::to_string(&GeminiSufficiencyResponse { sufficient: true, needed_items: vec![] })?));

    let markdown_content = r#"
File: src/file1_explicit.rs
```rust
// Content for file1
pub fn f1() {}
```

## src/file2_header.rs
```rust
// Content for file2
pub fn f2() {}
```

src/file3_simple.rs
```rust
// Content for file3
pub fn f3() {}
```
"#;

    let change = CodeChange {
        file_path: "MARKDOWN_CHANGES".to_string(),
        action: CodeChangeAction::ProcessMarkdownAndApplyChanges,
        content: Some(markdown_content.to_string()),
    };
    let code_gen_response = GeminiCodeGenerationResponse {
        changes: vec![change],
        tests: None,
        explanation: "Applied changes from Markdown with varied filename styles.".to_string(),
    };
    mock_api.add_mock_response(Ok(serde_json::to_string(&code_gen_response)?));

    let result = run_gem_logic_with_mock_api_owned(args, mock_api, project_root.clone());
    assert!(result.is_ok(), "run_gem_logic_with_mock_api_owned failed: {:?}", result.err());

    let file1_content = fs::read_to_string(project_root.join("src").join("file1_explicit.rs"))?;
    assert_eq!(file1_content.trim(), "// Content for file1\npub fn f1() {}");

    assert!(!project_root.join("src").join("file2_header.rs").exists(), "file2_header.rs should NOT have been created with the current regex");

    let file3_content = fs::read_to_string(project_root.join("src").join("file3_simple.rs"))?;
    assert_eq!(file3_content.trim(), "// Content for file3\npub fn f3() {}");

    Ok(())
}

#[test]
#[serial]
fn test_markdown_item_replacement_function_exists() -> Result<(), Box<dyn Error>> {
    let (project_root, _project_dir_guard, _home_dir_guard) = setup_test_env("md_item_replace_func");

    let initial_lib_content = r#"
pub fn func_to_replace() -> i32 { 1 }
pub fn another_func() {} // Should remain untouched
"#;
    fs::write(project_root.join("src").join("lib.rs"), initial_lib_content)?;

    let args = common_test_args(project_root.clone(), "Replace func_to_replace via markdown");

    let mut mock_api = MockLLMApi::new();
    mock_api.add_mock_response(Ok(serde_json::to_string(&GeminiNeededItemsResponse { needed_items: vec![] })?));
    mock_api.add_mock_response(Ok(serde_json::to_string(&GeminiSufficiencyResponse { sufficient: true, needed_items: vec![] })?));

    let markdown_content = r#"
src/lib.rs
```rust
pub fn func_to_replace() -> String { "replaced".to_string() }
```
"#;
    let change = CodeChange {
        file_path: "MARKDOWN_CHANGES".to_string(),
        action: CodeChangeAction::ProcessMarkdownAndApplyChanges,
        content: Some(markdown_content.to_string()),
    };
    let code_gen_response = GeminiCodeGenerationResponse {
        changes: vec![change],
        tests: None,
        explanation: "Replaced item func_to_replace from Markdown.".to_string(),
    };
    mock_api.add_mock_response(Ok(serde_json::to_string(&code_gen_response)?));

    let result = run_gem_logic_with_mock_api_owned(args, mock_api, project_root.clone());
    assert!(result.is_ok(), "run_gem_logic_with_mock_api_owned failed: {:?}", result.err());

    let modified_content = fs::read_to_string(project_root.join("src").join("lib.rs"))?;

    assert!(modified_content.contains("pub fn func_to_replace() -> String { \"replaced\".to_string() }"));
    assert!(modified_content.contains("pub fn another_func() {}"));
    assert!(!modified_content.contains("pub fn func_to_replace() -> i32 { 1 }"));

    Ok(())
}

#[test]
#[serial]
fn test_markdown_processing_no_valid_blocks() -> Result<(), Box<dyn Error>> {
    let (project_root, _project_dir_guard, _home_dir_guard) = setup_test_env("md_no_blocks");
    let initial_lib_rs_content = fs::read_to_string(project_root.join("src").join("lib.rs"))?;

    let args = common_test_args(project_root.clone(), "Process markdown with no valid blocks");

    let mut mock_api = MockLLMApi::new();
    mock_api.add_mock_response(Ok(serde_json::to_string(&GeminiNeededItemsResponse { needed_items: vec![] })?));
    mock_api.add_mock_response(Ok(serde_json::to_string(&GeminiSufficiencyResponse { sufficient: true, needed_items: vec![] })?));

    let markdown_content = "This is some Markdown text but it does not contain any valid file code blocks.";
    let change = CodeChange {
        file_path: "MARKDOWN_CHANGES".to_string(),
        action: CodeChangeAction::ProcessMarkdownAndApplyChanges,
        content: Some(markdown_content.to_string()),
    };
    let code_gen_response = GeminiCodeGenerationResponse {
        changes: vec![change],
        tests: None,
        explanation: "Processed Markdown with no valid blocks.".to_string(),
    };
    mock_api.add_mock_response(Ok(serde_json::to_string(&code_gen_response)?));

    let result = run_gem_logic_with_mock_api_owned(args, mock_api, project_root.clone());
    assert!(result.is_ok(), "run_gem_logic_with_mock_api_owned failed for markdown with no blocks: {:?}", result.err());

    let current_lib_rs_content = fs::read_to_string(project_root.join("src").join("lib.rs"))?;
    assert_eq!(current_lib_rs_content, initial_lib_rs_content, "lib.rs should be unchanged");

    let mut unexpected_files = Vec::new();
    for entry in fs::read_dir(project_root.join("src"))? {
        let entry = entry?;
        let file_name = entry.file_name().into_string().unwrap();
        if file_name != "lib.rs" {
            unexpected_files.push(file_name);
        }
    }
    assert!(unexpected_files.is_empty(), "Unexpected files found in src: {:?}", unexpected_files);

    Ok(())
}

#[test]
#[serial]
fn test_markdown_processing_empty_markdown() -> Result<(), Box<dyn Error>> {
    let (project_root, _project_dir_guard, _home_dir_guard) = setup_test_env("md_empty");
    let initial_lib_rs_content = fs::read_to_string(project_root.join("src").join("lib.rs"))?;

    let args = common_test_args(project_root.clone(), "Process empty markdown");

    let mut mock_api = MockLLMApi::new();
    mock_api.add_mock_response(Ok(serde_json::to_string(&GeminiNeededItemsResponse { needed_items: vec![] })?));
    mock_api.add_mock_response(Ok(serde_json::to_string(&GeminiSufficiencyResponse { sufficient: true, needed_items: vec![] })?));

    let markdown_content = "";
    let change = CodeChange {
        file_path: "MARKDOWN_CHANGES".to_string(),
        action: CodeChangeAction::ProcessMarkdownAndApplyChanges,
        content: Some(markdown_content.to_string()),
    };
    let code_gen_response = GeminiCodeGenerationResponse {
        changes: vec![change],
        tests: None,
        explanation: "Processed empty Markdown.".to_string(),
    };
    mock_api.add_mock_response(Ok(serde_json::to_string(&code_gen_response)?));

    let result = run_gem_logic_with_mock_api_owned(args, mock_api, project_root.clone());
    assert!(result.is_ok(), "run_gem_logic_with_mock_api_owned failed for empty markdown: {:?}", result.err());

    let current_lib_rs_content = fs::read_to_string(project_root.join("src").join("lib.rs"))?;
    assert_eq!(current_lib_rs_content, initial_lib_rs_content, "lib.rs should be unchanged");

    let mut unexpected_files = Vec::new();
    for entry in fs::read_dir(project_root.join("src"))? {
        let entry = entry?;
        let file_name = entry.file_name().into_string().unwrap();
        if file_name != "lib.rs" {
            unexpected_files.push(file_name);
        }
    }
    assert!(unexpected_files.is_empty(), "Unexpected files found in src: {:?}", unexpected_files);

    Ok(())
}

#[test]
#[serial]
fn test_markdown_processing_with_directory_creation() -> Result<(), Box<dyn Error>> {
    let (project_root, _project_dir_guard, _home_dir_guard) = setup_test_env("md_dir_creation");

    let args = common_test_args(project_root.clone(), "Create deeply nested file from markdown");

    let mut mock_api = MockLLMApi::new();
    mock_api.add_mock_response(Ok(serde_json::to_string(&GeminiNeededItemsResponse { needed_items: vec![] })?));
    mock_api.add_mock_response(Ok(serde_json::to_string(&GeminiSufficiencyResponse { sufficient: true, needed_items: vec![] })?));

    let markdown_content = r#"
File: src/deep/nested/module.rs
```rust
pub fn deep_func() {}
```
"#;
    let change = CodeChange {
        file_path: "MARKDOWN_CHANGES".to_string(),
        action: CodeChangeAction::ProcessMarkdownAndApplyChanges,
        content: Some(markdown_content.to_string()),
    };
    let code_gen_response = GeminiCodeGenerationResponse {
        changes: vec![change],
        tests: None,
        explanation: "Created a deeply nested module from Markdown.".to_string(),
    };
    mock_api.add_mock_response(Ok(serde_json::to_string(&code_gen_response)?));

    let result = run_gem_logic_with_mock_api_owned(args, mock_api, project_root.clone());
    assert!(result.is_ok(), "run_gem_logic_with_mock_api_owned failed: {:?}", result.err());

    let file_content = fs::read_to_string(project_root.join("src").join("deep").join("nested").join("module.rs"))?;
    let expected_file_content = "pub fn deep_func() {}";
    assert_eq!(file_content.trim(), expected_file_content.trim());
    assert!(project_root.join("src").join("deep").join("nested").is_dir());
    assert!(project_root.join("src").join("deep").is_dir());

    Ok(())
}

#[test]
#[serial]
fn test_markdown_processing_mixed_create_and_overwrite() -> Result<(), Box<dyn Error>> {
    let (project_root, _project_dir_guard, _home_dir_guard) = setup_test_env("md_mixed_ops");

    fs::write(project_root.join("src").join("lib.rs"), "// Old lib content")?;

    let args = common_test_args(project_root.clone(), "Mixed create and overwrite from markdown");

    let mut mock_api = MockLLMApi::new();
    mock_api.add_mock_response(Ok(serde_json::to_string(&GeminiNeededItemsResponse { needed_items: vec![] })?));
    mock_api.add_mock_response(Ok(serde_json::to_string(&GeminiSufficiencyResponse { sufficient: true, needed_items: vec![] })?));

    let markdown_content = r#"
src/lib.rs
```rust
// Updated lib content
```

src/newly_created.rs
```rust
// Content for a new file
pub const VALUE: i32 = 42;
```
"#;
    let change = CodeChange {
        file_path: "MARKDOWN_CHANGES".to_string(),
        action: CodeChangeAction::ProcessMarkdownAndApplyChanges,
        content: Some(markdown_content.to_string()),
    };
    let code_gen_response = GeminiCodeGenerationResponse {
        changes: vec![change],
        tests: None,
        explanation: "Applied mixed changes from Markdown.".to_string(),
    };
    mock_api.add_mock_response(Ok(serde_json::to_string(&code_gen_response)?));

    let result = run_gem_logic_with_mock_api_owned(args, mock_api, project_root.clone());
    assert!(result.is_ok(), "run_gem_logic_with_mock_api_owned failed: {:?}", result.err());

    let lib_content = fs::read_to_string(project_root.join("src").join("lib.rs"))?;
    let expected_lib_content = "// Updated lib content";
    assert_eq!(lib_content.trim(), expected_lib_content.trim());

    let new_file_content = fs::read_to_string(project_root.join("src").join("newly_created.rs"))?;
    let expected_new_file_content = "// Content for a new file\npub const VALUE: i32 = 42;";
    assert_eq!(new_file_content.trim(), expected_new_file_content.trim());

    Ok(())
}

#[test]
#[serial]
fn test_markdown_processing_overwrite_existing_files() -> Result<(), Box<dyn Error>> {
    let (project_root, _project_dir_guard, _home_dir_guard) = setup_test_env("md_overwrite_files");

    fs::write(project_root.join("src").join("lib.rs"), "pub fn old_lib_func() {}")?;
    fs::write(project_root.join("src").join("my_mod.rs"), "pub fn old_mod_func() {}")?;

    let args = common_test_args(project_root.clone(), "Overwrite files from markdown");

    let mut mock_api = MockLLMApi::new();
    mock_api.add_mock_response(Ok(serde_json::to_string(&GeminiNeededItemsResponse { needed_items: vec![] })?));
    mock_api.add_mock_response(Ok(serde_json::to_string(&GeminiSufficiencyResponse { sufficient: true, needed_items: vec![] })?));

    let markdown_content = r#"
src/lib.rs
```rust
// New lib content
pub fn new_lib_func() -> String { "new lib".to_string() }
```

File: src/my_mod.rs
```rust
// New mod content
pub fn new_mod_func(x: i32) -> i32 { x * 2 }
```
"#;
    let change = CodeChange {
        file_path: "MARKDOWN_CHANGES".to_string(),
        action: CodeChangeAction::ProcessMarkdownAndApplyChanges,
        content: Some(markdown_content.to_string()),
    };
    let code_gen_response = GeminiCodeGenerationResponse {
        changes: vec![change],
        tests: None,
        explanation: "Applied overwrites from Markdown.".to_string(),
    };
    mock_api.add_mock_response(Ok(serde_json::to_string(&code_gen_response)?));

    let result = run_gem_logic_with_mock_api_owned(args, mock_api, project_root.clone());
    assert!(result.is_ok(), "run_gem_logic_with_mock_api_owned failed: {:?}", result.err());

    let lib_content = fs::read_to_string(project_root.join("src").join("lib.rs"))?;
    let expected_lib_content = "// New lib content\npub fn new_lib_func() -> String { \"new lib\".to_string() }";
    assert_eq!(lib_content.trim(), expected_lib_content.trim());

    let mod_content = fs::read_to_string(project_root.join("src").join("my_mod.rs"))?;
    let expected_mod_content = "// New mod content\npub fn new_mod_func(x: i32) -> i32 { x * 2 }";
    assert_eq!(mod_content.trim(), expected_mod_content.trim());

    Ok(())
}


#[test]
#[serial]
fn test_markdown_processing_create_new_files() -> Result<(), Box<dyn Error>> {
    let (project_root, _project_dir_guard, _home_dir_guard) = setup_test_env("md_create_files");

    let initial_lib_rs_content = fs::read_to_string(project_root.join("src").join("lib.rs"))?;

    let args = common_test_args(project_root.clone(), "Create files from markdown");

    let mut mock_api = MockLLMApi::new();
    mock_api.add_mock_response(Ok(serde_json::to_string(&GeminiNeededItemsResponse { needed_items: vec![] })?));
    mock_api.add_mock_response(Ok(serde_json::to_string(&GeminiSufficiencyResponse { sufficient: true, needed_items: vec![] })?));

    let markdown_content = r#"
File: src/new_module_from_md.rs
```rust
pub fn func_one() { println!("one"); }
```

src/another_new_file.rs
```rust
pub struct Data { value: i32 }
```
"#;
    let change = CodeChange {
        file_path: "MARKDOWN_CHANGES".to_string(),
        action: CodeChangeAction::ProcessMarkdownAndApplyChanges,
        content: Some(markdown_content.to_string()),
    };
    let code_gen_response = GeminiCodeGenerationResponse {
        changes: vec![change],
        tests: None,
        explanation: "Applied changes from Markdown.".to_string(),
    };
    mock_api.add_mock_response(Ok(serde_json::to_string(&code_gen_response)?));

    let result = run_gem_logic_with_mock_api_owned(args, mock_api, project_root.clone());
    assert!(result.is_ok(), "run_gem_logic_with_mock_api_owned failed: {:?}", result.err());

    let file1_content = fs::read_to_string(project_root.join("src").join("new_module_from_md.rs"))?;
    let expected_file1_content = "pub fn func_one() { println!(\"one\"); }";
    assert_eq!(file1_content.trim(), expected_file1_content.trim());

    let file2_content = fs::read_to_string(project_root.join("src").join("another_new_file.rs"))?;
    let expected_file2_content = "pub struct Data { value: i32 }";
    assert_eq!(file2_content.trim(), expected_file2_content.trim());

    let current_lib_rs_content = fs::read_to_string(project_root.join("src").join("lib.rs"))?;
    assert_eq!(current_lib_rs_content, initial_lib_rs_content);


    Ok(())
}

#[test]
#[serial]
fn test_delete_file() -> Result<(), Box<dyn Error>> {
    let (project_root, _project_dir_guard, _home_dir_guard) = setup_test_env("delete_file");

    let file_to_delete_path_str = "src/to_be_deleted.rs";
    let file_to_delete_abs_path = project_root.join(file_to_delete_path_str);
    fs::write(&file_to_delete_abs_path, "fn useless_function() {}")?;
    assert!(file_to_delete_abs_path.exists());

    let initial_lib_content = fs::read_to_string(project_root.join("src").join("lib.rs"))?;

    let args = common_test_args(project_root.clone(), "delete to_be_deleted.rs");

    let mut mock_api = MockLLMApi::new();
    mock_api.add_mock_response(Ok(serde_json::to_string(&GeminiNeededItemsResponse { needed_items: vec![] })?));
    mock_api.add_mock_response(Ok(serde_json::to_string(&GeminiSufficiencyResponse { sufficient: true, needed_items: vec![] })?));

    let change = CodeChange {
        file_path: file_to_delete_path_str.to_string(),
        action: CodeChangeAction::DeleteFile,
        content: None,
    };
    let code_gen_response = GeminiCodeGenerationResponse {
        changes: vec![change],
        tests: None,
        explanation: "Deleted the specified file.".to_string(),
    };
    mock_api.add_mock_response(Ok(serde_json::to_string(&code_gen_response)?));

    let result = run_gem_logic_with_mock_api_owned(args, mock_api, project_root.clone());
    assert!(result.is_ok(), "run_gem_logic_with_mock_api_owned failed: {:?}", result.err());

    assert!(!file_to_delete_abs_path.exists(), "Expected file {:?} was not deleted", file_to_delete_abs_path);

    let current_lib_content = fs::read_to_string(project_root.join("src").join("lib.rs"))?;
    assert_eq!(current_lib_content, initial_lib_content);

    Ok(())
}

#[test]
#[serial]
fn test_create_file() -> Result<(), Box<dyn Error>> {
    let (project_root, _project_dir_guard, _home_dir_guard) = setup_test_env("create_file");

    let initial_lib_content = fs::read_to_string(project_root.join("src").join("lib.rs"))?;

    let args = common_test_args(project_root.clone(), "create a new module");

    let mut mock_api = MockLLMApi::new();
    mock_api.add_mock_response(Ok(serde_json::to_string(&GeminiNeededItemsResponse { needed_items: vec![] })?));
    mock_api.add_mock_response(Ok(serde_json::to_string(&GeminiSufficiencyResponse { sufficient: true, needed_items: vec![] })?));

    let new_module_path = "src/new_module.rs";
    let new_module_content = "pub fn newly_created_func() {\n    // Content of new module\n}";
    let change = CodeChange {
        file_path: new_module_path.to_string(),
        action: CodeChangeAction::CreateFile,
        content: Some(new_module_content.to_string()),
    };
    let code_gen_response = GeminiCodeGenerationResponse {
        changes: vec![change],
        tests: None,
        explanation: "Created a new module file.".to_string(),
    };
    mock_api.add_mock_response(Ok(serde_json::to_string(&code_gen_response)?));

    let result = run_gem_logic_with_mock_api_owned(args, mock_api, project_root.clone());
    assert!(result.is_ok(), "run_gem_logic_with_mock_api_owned failed: {:?}", result.err());

    let created_file_path = project_root.join(new_module_path);
    assert!(created_file_path.exists(), "Expected file {:?} was not created", created_file_path);
    let created_content = fs::read_to_string(created_file_path)?;
    assert_eq!(created_content.trim(), new_module_content.trim());

    let current_lib_content = fs::read_to_string(project_root.join("src").join("lib.rs"))?;
    assert_eq!(current_lib_content, initial_lib_content);

    Ok(())
}

#[test]
#[serial]
fn test_replace_content_entire_file() -> Result<(), Box<dyn Error>> {
    let (project_root, _project_dir_guard, _home_dir_guard) = setup_test_env("replace_entire_file");

    let initial_lib_content = r#"
fn old_function_to_be_wiped() {
    // ...
}
"#;
    fs::write(project_root.join("src").join("lib.rs"), initial_lib_content)?;

    let args = common_test_args(project_root.clone(), "replace entire lib.rs");

    let mut mock_api = MockLLMApi::new();
    mock_api.add_mock_response(Ok(serde_json::to_string(&GeminiNeededItemsResponse { needed_items: vec![] })?));
    mock_api.add_mock_response(Ok(serde_json::to_string(&GeminiSufficiencyResponse { sufficient: true, needed_items: vec![] })?));

    let new_total_content = "// Entirely new content\npub fn brand_new_function() {}";
    let change = CodeChange {
        file_path: "src/lib.rs".to_string(),
        action: CodeChangeAction::ReplaceContent,
        content: Some(new_total_content.to_string()),
    };
    let code_gen_response = GeminiCodeGenerationResponse {
        changes: vec![change],
        tests: None,
        explanation: "Replaced entire content of src/lib.rs.".to_string(),
    };
    mock_api.add_mock_response(Ok(serde_json::to_string(&code_gen_response)?));

    let result = run_gem_logic_with_mock_api_owned(args, mock_api, project_root.clone());
    assert!(result.is_ok(), "run_gem_logic_with_mock_api_owned failed: {:?}", result.err());

    let modified_lib_content = fs::read_to_string(project_root.join("src").join("lib.rs"))?;
    assert_eq!(modified_lib_content.trim(), new_total_content.trim());
    assert!(!modified_lib_content.contains("old_function_to_be_wiped"));

    Ok(())
}

#[test]
#[serial]
fn test_replace_item_not_found() -> Result<(), Box<dyn Error>> {
    let (project_root, _project_dir_guard, _home_dir_guard) = setup_test_env("item_not_found");

    let initial_lib_content = r#"
fn some_func() {
    // original content
}
"#;
    fs::write(project_root.join("src").join("lib.rs"), initial_lib_content)?;

    let args = common_test_args(project_root.clone(), "update non_existent_func");

    let mut mock_api = MockLLMApi::new();
    mock_api.add_mock_response(Ok(serde_json::to_string(&GeminiNeededItemsResponse { needed_items: vec![] })?));
    mock_api.add_mock_response(Ok(serde_json::to_string(&GeminiSufficiencyResponse { sufficient: true, needed_items: vec![] })?));

    let change = CodeChange {
        file_path: "src/lib.rs::non_existent_func".to_string(),
        action: CodeChangeAction::ReplaceItemInSection,
        content: Some("fn new_func() {}".to_string()),
    };
    let code_gen_response = GeminiCodeGenerationResponse {
        changes: vec![change],
        tests: None,
        explanation: "Attempting to update a non-existent function.".to_string(),
    };
    mock_api.add_mock_response(Ok(serde_json::to_string(&code_gen_response)?));

    let result = run_gem_logic_with_mock_api_owned(args, mock_api, project_root.clone());

    assert!(result.is_err(), "Expected run_gem_logic_with_mock_api_owned to fail for non-existent item.");
    if let Some(err) = result.err() {
        let err_msg = err.to_string();
        assert!(err_msg.contains("Failed to find item 'non_existent_func'"), "Error message did not indicate item not found. Actual: {}", err_msg);
    }

    let current_lib_content = fs::read_to_string(project_root.join("src").join("lib.rs"))?;
    assert_eq!(current_lib_content.trim(), initial_lib_content.trim());

    Ok(())
}

#[test]
#[serial]
fn test_replace_item_in_module() -> Result<(), Box<dyn Error>> {
    let (project_root, _project_dir_guard, _home_dir_guard) = setup_test_env("replace_in_module");

    let mod_content = r#"
pub fn func_in_mod() {
    // original content
}
"#;
    let mod_dir = project_root.join("src").join("my_mod.rs");
    fs::write(&mod_dir, mod_content)?;

    let lib_content = r#"
mod my_mod;
pub fn lib_func() {}
"#;
    fs::write(project_root.join("src").join("lib.rs"), lib_content)?;

    let args = common_test_args(project_root.clone(), "update func_in_mod");

    let mut mock_api = MockLLMApi::new();
    mock_api.add_mock_response(Ok(serde_json::to_string(&GeminiNeededItemsResponse { needed_items: vec![] })?));
    mock_api.add_mock_response(Ok(serde_json::to_string(&GeminiSufficiencyResponse { sufficient: true, needed_items: vec![] })?));

    let new_func_content = "pub fn changed_func_in_mod() -> i32 {\n    100\n}";
    let change = CodeChange {
        file_path: "src/my_mod.rs::func_in_mod".to_string(),
        action: CodeChangeAction::ReplaceItemInSection,
        content: Some(new_func_content.to_string()),
    };
    let code_gen_response = GeminiCodeGenerationResponse {
        changes: vec![change],
        tests: None,
        explanation: "Updated func_in_mod.".to_string(),
    };
    mock_api.add_mock_response(Ok(serde_json::to_string(&code_gen_response)?));

    let result = run_gem_logic_with_mock_api_owned(args, mock_api, project_root.clone());
    assert!(result.is_ok(), "run_gem_logic_with_mock_api_owned failed: {:?}", result.err());

    let modified_mod_content = fs::read_to_string(&mod_dir)?;
    let expected_mod_content = r#"
pub fn changed_func_in_mod() -> i32 {
    100
}
"#;
    assert_eq!(modified_mod_content.trim(), expected_mod_content.trim());
    assert!(!modified_mod_content.contains("original content"));

    let current_lib_content = fs::read_to_string(project_root.join("src").join("lib.rs"))?;
    assert_eq!(current_lib_content.trim(), lib_content.trim());

    Ok(())
}

#[test]
#[serial]
fn test_replace_item_in_section_enum() -> Result<(), Box<dyn Error>> {
    let (project_root, _project_dir_guard, _home_dir_guard) = setup_test_env("replace_enum");

    let initial_lib_content = r#"
enum MyEnum {
    OldVariant,
}
const SOME_CONST: i32 = 42;
"#;
    fs::write(project_root.join("src").join("lib.rs"), initial_lib_content)?;

    let args = common_test_args(project_root.clone(), "update MyEnum");

    let mut mock_api = MockLLMApi::new();
    mock_api.add_mock_response(Ok(serde_json::to_string(&GeminiNeededItemsResponse { needed_items: vec![] })?));
    mock_api.add_mock_response(Ok(serde_json::to_string(&GeminiSufficiencyResponse { sufficient: true, needed_items: vec![] })?));

    let new_enum_content = "enum MyEnumUpdated {\n    NewVariantA,\n    NewVariantB,\n}";
    let change = CodeChange {
        file_path: "src/lib.rs::MyEnum".to_string(),
        action: CodeChangeAction::ReplaceItemInSection,
        content: Some(new_enum_content.to_string()),
    };
    let code_gen_response = GeminiCodeGenerationResponse {
        changes: vec![change],
        tests: None,
        explanation: "Updated MyEnum.".to_string(),
    };
    mock_api.add_mock_response(Ok(serde_json::to_string(&code_gen_response)?));

    let result = run_gem_logic_with_mock_api_owned(args, mock_api, project_root.clone());
    assert!(result.is_ok(), "run_gem_logic_with_mock_api_owned failed: {:?}", result.err());

    let modified_lib_content = fs::read_to_string(project_root.join("src").join("lib.rs"))?;
    let expected_lib_content = r#"
enum MyEnumUpdated {
    NewVariantA,
    NewVariantB,
}
const SOME_CONST: i32 = 42;
"#;
    assert_eq!(modified_lib_content.trim(), expected_lib_content.trim());
    assert!(!modified_lib_content.contains("OldVariant"));
    assert!(modified_lib_content.contains("MyEnumUpdated"));

    Ok(())
}

#[test]
#[serial]
fn test_replace_item_in_section_struct() -> Result<(), Box<dyn Error>> {
    let (project_root, _project_dir_guard, _home_dir_guard) = setup_test_env("replace_struct");

    let initial_lib_content = r#"
struct MyStruct {
    field: i32,
}
fn another_func() {}
"#;
    fs::write(project_root.join("src").join("lib.rs"), initial_lib_content)?;

    let args = common_test_args(project_root.clone(), "update MyStruct");

    let mut mock_api = MockLLMApi::new();
    mock_api.add_mock_response(Ok(serde_json::to_string(&GeminiNeededItemsResponse { needed_items: vec![] })?));
    mock_api.add_mock_response(Ok(serde_json::to_string(&GeminiSufficiencyResponse { sufficient: true, needed_items: vec![] })?));

    let new_struct_content = "struct MyStructUpdated {\n    new_field: String,\n}";
    let change = CodeChange {
        file_path: "src/lib.rs::MyStruct".to_string(),
        action: CodeChangeAction::ReplaceItemInSection,
        content: Some(new_struct_content.to_string()),
    };
    let code_gen_response = GeminiCodeGenerationResponse {
        changes: vec![change],
        tests: None,
        explanation: "Updated MyStruct.".to_string(),
    };
    mock_api.add_mock_response(Ok(serde_json::to_string(&code_gen_response)?));

    let result = run_gem_logic_with_mock_api_owned(args, mock_api, project_root.clone());
    assert!(result.is_ok(), "run_gem_logic_with_mock_api_owned failed: {:?}", result.err());

    let modified_lib_content = fs::read_to_string(project_root.join("src").join("lib.rs"))?;
    let expected_lib_content = r#"
struct MyStructUpdated {
    new_field: String,
}
fn another_func() {}
"#;
    assert_eq!(modified_lib_content.trim(), expected_lib_content.trim());
    assert!(!modified_lib_content.contains("field: i32"));
    assert!(modified_lib_content.contains("MyStructUpdated"));

    Ok(())
}

#[test]
#[serial]
fn test_code_generation_flow_applies_changes() -> Result<(), Box<dyn Error>> {
    let (project_root, _project_dir_guard, _home_dir_guard) = setup_test_env("code_gen_applies_changes");

    let args = common_test_args(project_root.clone(), "create a new file");

    let mut mock_api = MockLLMApi::new();

    let initial_response = GeminiNeededItemsResponse { needed_items: vec![] };
    mock_api.add_mock_response(Ok(serde_json::to_string(&initial_response)?));

    let sufficiency_response = GeminiSufficiencyResponse { sufficient: true, needed_items: vec![] };
    mock_api.add_mock_response(Ok(serde_json::to_string(&sufficiency_response)?));

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

    let result = run_gem_logic_with_mock_api_owned(args, mock_api, project_root.clone());
    assert!(result.is_ok(), "run_gem_logic_with_mock_api_owned failed: {:?}", result.err());

    let expected_file = project_root.join(new_file_path);
    assert!(expected_file.exists(), "Expected file {:?} was not created", expected_file);
    assert_eq!(fs::read_to_string(expected_file)?, new_file_content);

    Ok(())
}

#[test]
#[serial]
fn test_sufficiency_loop_sufficient_case() -> Result<(), Box<dyn Error>> {
    let (project_root, _project_dir_guard, _home_dir_guard) = setup_test_env("sufficiency_sufficient");

    let args = common_test_args(project_root.clone(), "test sufficiency: sufficient case");

    let mut mock_api = MockLLMApi::new();

    let initial_response = GeminiNeededItemsResponse { needed_items: vec!["src/lib.rs".to_string()] };
    mock_api.add_mock_response(Ok(serde_json::to_string(&initial_response)?));

    let sufficiency_response = GeminiSufficiencyResponse { sufficient: true, needed_items: vec![] };
    mock_api.add_mock_response(Ok(serde_json::to_string(&sufficiency_response)?));

    let change_response = gem::llm_api::GeminiCodeGenerationResponse {
        changes: vec![],
        tests: None,
        explanation: "Proceeded to code generation as data was sufficient.".to_string(),
    };
    mock_api.add_mock_response(Ok(serde_json::to_string(&change_response)?));

    let result = run_gem_logic_with_mock_api_owned(args, mock_api, project_root);

    assert!(result.is_ok(), "run_gem_logic_with_mock_api_owned failed: {:?}", result.err());

    Ok(())
}

#[test]
#[serial]
fn test_replace_item_in_section_const() -> Result<(), Box<dyn Error>> {
    let (project_root, _project_dir_guard, _home_dir_guard) = setup_test_env("replace_const");

    let initial_lib_content = r#"
const OLD_CONST: i32 = 1;
fn some_func_for_context() {}
"#;
    fs::write(project_root.join("src").join("lib.rs"), initial_lib_content)?;

    let args = common_test_args(project_root.clone(), "update OLD_CONST");

    let mut mock_api = MockLLMApi::new();
    mock_api.add_mock_response(Ok(serde_json::to_string(&GeminiNeededItemsResponse { needed_items: vec![] })?));
    mock_api.add_mock_response(Ok(serde_json::to_string(&GeminiSufficiencyResponse { sufficient: true, needed_items: vec![] })?));

    let new_const_content = "const NEW_CONST: &str = \"hello\";";
    let change = CodeChange {
        file_path: "src/lib.rs::OLD_CONST".to_string(),
        action: CodeChangeAction::ReplaceItemInSection,
        content: Some(new_const_content.to_string()),
    };
    let code_gen_response = GeminiCodeGenerationResponse {
        changes: vec![change],
        tests: None,
        explanation: "Updated OLD_CONST.".to_string(),
    };
    mock_api.add_mock_response(Ok(serde_json::to_string(&code_gen_response)?));

    let result = run_gem_logic_with_mock_api_owned(args, mock_api, project_root.clone());
    assert!(result.is_ok(), "run_gem_logic_with_mock_api_owned failed: {:?}", result.err());

    let modified_lib_content = fs::read_to_string(project_root.join("src").join("lib.rs"))?;
    let expected_lib_content = r#"
const NEW_CONST: &str = "hello";
fn some_func_for_context() {}
"#;
    assert_eq!(modified_lib_content.trim(), expected_lib_content.trim());
    assert!(!modified_lib_content.contains("OLD_CONST"));
    assert!(modified_lib_content.contains("NEW_CONST"));

    Ok(())
}

#[test]
#[serial]
fn test_replace_item_in_section_function() -> Result<(), Box<dyn Error>> {
    let (project_root, _project_dir_guard, _home_dir_guard) = setup_test_env("replace_func");

    let initial_lib_content = r#"
// Some comment
fn old_func() -> i32 {
    1
}
// Another comment
"#;
    fs::write(project_root.join("src").join("lib.rs"), initial_lib_content)?;

    let args = common_test_args(project_root.clone(), "replace old_func with new_func");

    let mut mock_api = MockLLMApi::new();

    mock_api.add_mock_response(Ok(serde_json::to_string(&GeminiNeededItemsResponse { needed_items: vec![] })?));
    mock_api.add_mock_response(Ok(serde_json::to_string(&GeminiSufficiencyResponse { sufficient: true, needed_items: vec![] })?));

    let new_function_content = "fn new_func() -> i32 {\n    2\n}";
    let change = CodeChange {
        file_path: "src/lib.rs::old_func".to_string(),
        action: CodeChangeAction::ReplaceItemInSection,
        content: Some(new_function_content.to_string()),
    };
    let code_gen_response = GeminiCodeGenerationResponse {
        changes: vec![change],
        tests: None,
        explanation: "Replaced old_func with new_func.".to_string(),
    };
    mock_api.add_mock_response(Ok(serde_json::to_string(&code_gen_response)?));

    let result = run_gem_logic_with_mock_api_owned(args, mock_api, project_root.clone());
    assert!(result.is_ok(), "run_gem_logic_with_mock_api_owned failed: {:?}", result.err());

    let modified_lib_content = fs::read_to_string(project_root.join("src").join("lib.rs"))?;
    let expected_lib_content = r#"
// Some comment
fn new_func() -> i32 {
    2
}
// Another comment
"#;
    assert_eq!(modified_lib_content.trim(), expected_lib_content.trim());
    assert!(!modified_lib_content.contains("old_func"));

    Ok(())
}

#[test]
#[serial]
fn test_markdown_replace_existing_function() -> Result<(), Box<dyn Error>> {
    let (project_root, _project_dir_guard, _home_dir_guard) = setup_test_env("md_replace_func_specific");

    let initial_lib_content = r#"
// Initial content
pub fn function_to_replace() -> i32 {
    1 // old content
}

pub fn another_function() {
    // this should remain
}
"#;
    fs::write(project_root.join("src").join("lib.rs"), initial_lib_content)?;

    let args = common_test_args(project_root.clone(), "Replace function_to_replace with new content via Markdown");

    let mut mock_api = MockLLMApi::new();
    mock_api.add_mock_response(Ok(serde_json::to_string(&GeminiNeededItemsResponse { needed_items: vec![] })?));
    mock_api.add_mock_response(Ok(serde_json::to_string(&GeminiSufficiencyResponse { sufficient: true, needed_items: vec![] })?));

    let markdown_content = r#"
This is an explanation of the change.
We are replacing `function_to_replace`.

File: src/lib.rs
```rust
pub fn function_to_replace() -> String {
    "new content".to_string()
}
```

The function signature and body have been updated.
"#;

    let change = CodeChange {
        file_path: "MARKDOWN_CHANGES".to_string(),
        action: CodeChangeAction::ProcessMarkdownAndApplyChanges,
        content: Some(markdown_content.to_string()),
    };
    let llm_response = GeminiCodeGenerationResponse {
        changes: vec![change],
        tests: None,
        explanation: "High-level: Updated a function via Markdown.".to_string(),
    };
    mock_api.add_mock_response(Ok(serde_json::to_string(&llm_response)?));

    let _session = run_gem_logic_with_mock_api_owned(args, mock_api, project_root.clone())?;

    let modified_content = fs::read_to_string(project_root.join("src").join("lib.rs"))?;

    assert!(modified_content.contains("pub fn function_to_replace() -> String {"));
    assert!(modified_content.contains("\"new content\".to_string()"));
    assert!(!modified_content.contains("-> i32"));
    assert!(!modified_content.contains("1 // old content"));
    assert!(modified_content.contains("pub fn another_function()"));

    Ok(())
}

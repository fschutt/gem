#[cfg(test)]
mod tests {
    use gem::run_gem_agent;
    use gem::cli::CustomCliArgs;
    use gem::cache::Session;
    use gem::llm_api::{RealLLMApi, LLMApi}; // LLMApi might not be needed if just using RealLLMApi

    use std::path::PathBuf;
    use tempfile::{tempdir, TempDir};
    use std::fs;
    use std::error::Error;
    use std::process::Command; // For running cargo test externally

    // Re-using a setup similar to llm_integration_tests.rs
    // Returns project_root, guard for project_root, guard for home_dir
    fn setup_test_project_env(test_name: &str) -> (PathBuf, TempDir, TempDir) {
        let temp_project_dir_guard = tempdir().unwrap();
        let project_root = temp_project_dir_guard.path().to_path_buf();

        let src_dir = project_root.join("src");
        fs::create_dir_all(&src_dir).unwrap();
        // Start with a very basic lib.rs
        fs::write(src_dir.join("lib.rs"), "// Initial empty lib.rs\n").unwrap();
        fs::write(
            project_root.join("Cargo.toml"),
            "[package]\nname = \"live_test_project\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\n",
        )
        .unwrap();

        let temp_home_dir_guard = tempdir().unwrap();
        let home_path_str = temp_home_dir_guard.path().to_str().unwrap().to_string();
        std::env::set_var("HOME", &home_path_str);

        // Session directory will be created by Session::new under this mocked HOME
        // based on a hash of CLI args.

        (project_root, temp_project_dir_guard, temp_home_dir_guard)
    }

    #[test]
    #[ignore] // Ignoring because it makes real API calls and can be slow/flaky/require valid API key
    fn test_live_llm_create_simple_function() -> Result<(), Box<dyn Error>> {
        let (project_root, _project_dir_guard, _home_dir_guard) =
            setup_test_project_env("live_create_simple_fn");

        let mut args = CustomCliArgs::default();
        args.user_request = "add a public function named `say_hello` to `src/lib.rs` that takes no arguments and returns the string \"hello live\". Also add a test for it.".to_string();
        args.project_root = project_root.clone();
        args.verify_with = "cargo test".to_string(); // Use cargo test for verification
        args.no_test = false; // We want it to generate tests
        // args.max_data_loops = 1; // Potentially limit loops for a simple task
        // args.max_verify_retries = 1;


        // API key will be picked up from env var GEM_KEY or the fallback by RealLLMApi/run_gem_agent.
        // Ensure GEM_KEY is set in the test environment or rely on the fallback.
        // For this test, we rely on the fallback key "AIzaSyDRuWmnT1X7VC_Ur-EkTqr62jrdQ78GDsw"
        // which is hardcoded in run_gem_agent if GEM_KEY is not found and not in debug_mode.
        // Note: run_gem_agent's API key logic is in main.rs before calling run_gem_agent.
        // The RealLLMApi in lib.rs takes the key directly.
        // The main.rs creates RealLLMApi with the key.
        // For a direct test like this, we need to simulate how main.rs sets up RealLLMApi.

        let api_key = std::env::var("GEM_KEY").unwrap_or_else(|_| {
            println!("NOTE: GEM_KEY not set, using fallback for live test_live_llm_create_simple_function. Ensure fallback is active or set GEM_KEY.");
            "AIzaSyDRuWmnT1X7VC_Ur-EkTqr62jrdQ78GDsw".to_string()
        });


        let llm_api = Box::new(RealLLMApi::new(api_key));

        let session_id_str = format!("{:?}", args);
        let session_id = Session::compute_hash(&session_id_str);
        let mut session = Session::new(&session_id);

        // Run the agent
        let run_result = run_gem_agent(args, &mut session, llm_api, false /* is_interactive */, project_root.clone());

        // Check if the agent reported success
        if let Err(e) = &run_result {
            eprintln!("Live LLM test failed. Error from run_gem_agent: {}", e);
        }
        assert!(run_result.is_ok(), "run_gem_agent failed due to reasons logged above or an API issue.");

        // Verification: Check file content
        let lib_rs_path = project_root.join("src").join("lib.rs");
        assert!(lib_rs_path.exists(), "src/lib.rs was not created or was removed.");
        let lib_content = fs::read_to_string(&lib_rs_path)?;

        println!("--- src/lib.rs content ---");
        println!("{}", lib_content);
        println!("--- EOF src/lib.rs ---");

        assert!(lib_content.contains("pub fn say_hello()"), "Function `say_hello` not found in src/lib.rs");
        assert!(lib_content.contains("\"hello live\""), "Return string \"hello live\" not found in src/lib.rs");
        assert!(lib_content.contains("#[cfg(test)]"), "Tests section not found");
        assert!(lib_content.contains("assert_eq!(say_hello(), \"hello live\");"), "Test assertion not found");

        // Verification: Run cargo test in the temporary project
        println!("Running `cargo test` in temporary project: {:?}", project_root);
        let cargo_test_output = Command::new("cargo")
            .arg("test")
            .arg("--manifest-path")
            .arg(project_root.join("Cargo.toml"))
            .output()?;

        println!("`cargo test` stdout:\n{}", String::from_utf8_lossy(&cargo_test_output.stdout));
        eprintln!("`cargo test` stderr:\n{}", String::from_utf8_lossy(&cargo_test_output.stderr));

        assert!(cargo_test_output.status.success(), "`cargo test` in the temporary project failed.");

        Ok(())
    }
}

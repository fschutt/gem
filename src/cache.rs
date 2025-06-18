use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::io;
use std::path::PathBuf;

/// Session manages both caching and persistent state across requests
pub struct Session {
    #[allow(dead_code)] // id is planned for future use (e.g., session resumption, logging)
    id: String,
    session_dir: PathBuf,
    pub gathered_data: HashMap<String, String>,
    in_memory_cache: HashMap<String, String>, // Stores prompt hash -> response
    prompts: HashMap<String, String>,         // Stores prompt_type-hash -> prompt text
}

impl Session {
    pub fn new(session_id: &str) -> Self {
        let home_dir = env::var("HOME").unwrap_or_else(|_| {
            if cfg!(windows) {
                env::var("USERPROFILE").unwrap_or_else(|_| ".".to_string())
            } else {
                ".".to_string()
            }
        });

        let base_dir = PathBuf::from(home_dir).join(".gem");
        let session_dir = base_dir.join("session").join(session_id);
        fs::create_dir_all(&session_dir).expect("Failed to create session directory");

        let mut gathered_data = HashMap::new();
        let data_file = session_dir.join("data.json");
        if data_file.exists() {
            if let Ok(data_json) = fs::read_to_string(&data_file) {
                if let Ok(loaded_data) = serde_json::from_str::<HashMap<String, String>>(&data_json) {
                    gathered_data = loaded_data;
                }
            }
        }

        let mut in_memory_cache = HashMap::new();
        let mut prompts = HashMap::new();

        if let Ok(entries) = fs::read_dir(&session_dir) {
            for entry in entries.filter_map(Result::ok) {
                let path = entry.path();
                if path.is_file() {
                    if let Some(filename_str) = path.file_name().and_then(|n| n.to_str()) {
                        if filename_str.contains("-response-") && filename_str.ends_with(".txt") {
                            if let Some(hash) = filename_str.split("-response-").last().and_then(|s| s.strip_suffix(".txt")) {
                                if let Ok(content) = fs::read_to_string(&path) {
                                    in_memory_cache.insert(hash.to_string(), content);
                                }
                            }
                        } else if !filename_str.contains("-response-")
                            && filename_str.ends_with(".txt")
                            && filename_str != "data.json"
                        {
                            let parts: Vec<&str> = filename_str.splitn(2, '-').collect();
                            if parts.len() == 2 {
                                if let Some(hash_candidate) = parts[1].strip_suffix(".txt") {
                                    if hash_candidate.len() == 64 && hash_candidate.chars().all(|c| c.is_ascii_hexdigit()) {
                                        if let Ok(content) = fs::read_to_string(&path) {
                                            prompts.insert(format!("{}-{}", parts[0], hash_candidate), content);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        Self {
            id: session_id.to_string(),
            session_dir,
            gathered_data,
            in_memory_cache,
            prompts,
        }
    }

    /// Compute SHA256 hash of input string
    pub fn compute_hash(input: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(input.as_bytes());
        let result = hasher.finalize();
        format!("{:x}", result)
    }

    /// Add or update data in the session
    pub fn add_data(&mut self, key: &str, value: &str) {
        self.gathered_data
            .insert(key.to_string(), value.to_string());
    }

    /// Save session data
    pub fn save(&self) -> io::Result<()> {
        // Save gathered data to data.json
        let data_file = self.session_dir.join("data.json");
        let data_json = serde_json::to_string(&self.gathered_data)?;
        fs::write(data_file, data_json)?;

        Ok(())
    }

    /// Get cached response for a given prompt
    pub fn get_cached_response(&self, _prompt_type: &str, content: &str) -> Option<String> {
        let hash = Self::compute_hash(content);
        self.in_memory_cache.get(&hash).cloned()
    }

    /// Save a prompt and its response
    pub fn save_prompt_and_response(
        &mut self,
        prompt_type: &str,
        prompt: &str,
        response: &str,
    ) -> io::Result<()> {
        let hash = Self::compute_hash(prompt);

        // Update in-memory caches
        self.prompts
            .insert(format!("{}-{}", prompt_type, hash), prompt.to_string());
        self.in_memory_cache
            .insert(hash.clone(), response.to_string());

        // Save the prompt to file
        let prompt_file = self
            .session_dir
            .join(format!("{}-{}.txt", prompt_type, hash));
        fs::write(&prompt_file, prompt)?;

        // Save the response to file
        let response_file = self
            .session_dir
            .join(format!("{}-response-{}.txt", prompt_type, hash));
        fs::write(&response_file, response)?;

        Ok(())
    }

    pub fn load_mock_cache(&mut self, mock_data: HashMap<String, String>) {
        self.in_memory_cache.extend(mock_data);
    }

    pub fn load_mock_prompts(&mut self, mock_prompts: HashMap<String, String>) {
        self.prompts.extend(mock_prompts);
    }

    /// Append to a prompt file (for accumulating sufficiency checks)
    // This method seems to be for specific, non-hashed prompts like 'initial.txt'.
    // It might need rethinking if these also need to be part of the hashed prompt/response flow.
    // For now, keeping its file system interaction direct.
    pub fn overwrite_prompt(&self, prompt_type: &str, content: &str) -> io::Result<()> {
        let prompt_file = self.session_dir.join(format!("{}.txt", prompt_type));
        fs::write(&prompt_file, content)?;
        Ok(())
    }

    /// Append to a prompt file (for accumulating sufficiency checks)
    // Similar to overwrite_prompt, this interacts directly with FS for specific prompt types.
    pub fn append_to_prompt(&self, prompt_type: &str, content: &str) -> io::Result<()> {
        let prompt_file = self.session_dir.join(format!("{}.txt", prompt_type));

        // Create or append to the file
        let current_content = if prompt_file.exists() {
            fs::read_to_string(&prompt_file)?
        } else {
            String::new()
        };

        let updated_content = if current_content.is_empty() {
            content.to_string()
        } else {
            format!("{}\n\n{}", current_content, content)
        };

        fs::write(&prompt_file, updated_content)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::{tempdir, TempDir};
    use serial_test::serial;

    fn setup_session(session_id: &str) -> (Session, PathBuf, TempDir) {
        let temp_dir = tempdir().unwrap();
        let home_path_for_session = temp_dir.path().to_path_buf();
        let base_gem_dir = home_path_for_session.join(".gem");
        let session_dir = base_gem_dir.join("session").join(session_id);
        fs::create_dir_all(&session_dir).unwrap();

        // Override HOME env var to use temp_dir for Session::new
        std::env::set_var("HOME", home_path_for_session.to_str().unwrap());

        (Session::new(session_id), session_dir.clone(), temp_dir)
    }

    #[test]
    #[serial]
    fn test_session_new_loads_cache_from_files() {
        let session_id = "test_load_session";
        let (_session, session_dir, _temp_dir_guard) = setup_session(session_id);

        // Create dummy cache files
        let prompt1_hash = Session::compute_hash("prompt1 text");
        let prompt2_hash = Session::compute_hash("prompt2 text");

        fs::write(session_dir.join(format!("typeA-{}.txt", prompt1_hash)), "prompt1 text").unwrap();
        fs::write(session_dir.join(format!("typeA-response-{}.txt", prompt1_hash)), "response1 text").unwrap();
        fs::write(session_dir.join(format!("typeB-{}.txt", prompt2_hash)), "prompt2 text").unwrap();
        fs::write(session_dir.join(format!("typeB-response-{}.txt", prompt2_hash)), "response2 text").unwrap();
        fs::write(session_dir.join("data.json"), r#"{"key1":"data1"}"#).unwrap();
        fs::write(session_dir.join("initial.txt"), "some initial prompt").unwrap(); // Should be ignored by prompt/response loading


        // Create a new session, it should load from files
        let loaded_session = Session::new(session_id);

        assert_eq!(loaded_session.in_memory_cache.get(&prompt1_hash), Some(&"response1 text".to_string()));
        assert_eq!(loaded_session.in_memory_cache.get(&prompt2_hash), Some(&"response2 text".to_string()));
        assert_eq!(loaded_session.prompts.get(&format!("typeA-{}",prompt1_hash)), Some(&"prompt1 text".to_string()));
        assert_eq!(loaded_session.prompts.get(&format!("typeB-{}",prompt2_hash)), Some(&"prompt2 text".to_string()));
        assert_eq!(loaded_session.gathered_data.get("key1"), Some(&"data1".to_string()));
        assert_eq!(loaded_session.in_memory_cache.len(), 2);
        assert_eq!(loaded_session.prompts.len(), 2);
    }

    #[test]
    #[serial]
    fn test_save_and_get_cached_response() {
        let session_id = "test_save_get_session";
        let (mut session, session_dir, _temp_dir_guard) = setup_session(session_id);

        let prompt_type = "test_type";
        let prompt_text = "This is a test prompt.";
        let response_text = "This is a test response.";
        let prompt_hash = Session::compute_hash(prompt_text);

        // Save
        session.save_prompt_and_response(prompt_type, prompt_text, response_text).unwrap();

        // Check in-memory cache
        assert_eq!(session.in_memory_cache.get(&prompt_hash), Some(&response_text.to_string()));
        assert_eq!(session.prompts.get(&format!("{}-{}", prompt_type, prompt_hash)), Some(&prompt_text.to_string()));

        // Check get_cached_response
        assert_eq!(session.get_cached_response(prompt_type, prompt_text), Some(response_text.to_string()));

        // Check files were written
        let prompt_file_path = session_dir.join(format!("{}-{}.txt", prompt_type, prompt_hash));
        let response_file_path = session_dir.join(format!("{}-response-{}.txt", prompt_type, prompt_hash));
        assert!(prompt_file_path.exists());
        assert_eq!(fs::read_to_string(prompt_file_path).unwrap(), prompt_text);
        assert!(response_file_path.exists());
        assert_eq!(fs::read_to_string(response_file_path).unwrap(), response_text);
    }

    #[test]
    #[serial]
    fn test_get_cached_response_miss() {
        let session_id = "test_cache_miss_session";
        let (session, _session_dir, _temp_dir_guard) = setup_session(session_id);
        assert_eq!(session.get_cached_response("any_type", "non_existent_prompt"), None);
    }

    #[test]
    #[serial]
    fn test_load_mock_cache_and_prompts() {
        let session_id = "test_mock_load_session";
        let (mut session, _session_dir, _temp_dir_guard) = setup_session(session_id);

        let mut mock_cache = HashMap::new();
        mock_cache.insert("hash123".to_string(), "mocked_response_1".to_string());
        session.load_mock_cache(mock_cache);
        assert_eq!(session.in_memory_cache.get("hash123"), Some(&"mocked_response_1".to_string()));

        let mut mock_prompts = HashMap::new();
        mock_prompts.insert("typeC-hash456".to_string(), "mocked_prompt_A".to_string());
        session.load_mock_prompts(mock_prompts);
        assert_eq!(session.prompts.get("typeC-hash456"), Some(&"mocked_prompt_A".to_string()));
    }

    #[test]
    #[serial]
    fn test_session_new_empty_dir() {
        let session_id = "test_empty_dir_session";
        // _td_guard keeps the temp directory alive for the Session::new call within setup_session
        let (_session_placeholder, session_dir_path, _td_guard) = setup_session(session_id);

        // Ensure data.json is not there or empty for this test.
        // setup_session already creates an empty session dir. Session::new called inside it
        // will try to load data.json. If it's not there, gathered_data will be empty.
        // If it is there (e.g. from a previous failed test run using the same session_id, though unlikely with temp dirs),
        // we remove it to ensure a truly empty state for gathered_data loading.
        let data_file = session_dir_path.join("data.json");
        if data_file.exists() {
            fs::remove_file(data_file).unwrap();
        }

        // Now, create a new Session instance targeting this specific, now-confirmed-empty directory.
        // The HOME environment variable is still set from the setup_session call.
        let session_loaded_from_empty_dir = Session::new(session_id);

        assert!(session_loaded_from_empty_dir.in_memory_cache.is_empty());
        assert!(session_loaded_from_empty_dir.prompts.is_empty());
        assert!(session_loaded_from_empty_dir.gathered_data.is_empty());
    }

    #[test]
    #[serial]
    fn test_session_new_ignores_unrelated_files() {
        let session_id = "test_ignore_files_session";
        let (_session, session_dir, _temp_dir_guard) = setup_session(session_id);

        fs::write(session_dir.join("random_file.txt"), "random_content").unwrap();
        fs::write(session_dir.join("another-file-no-response.dat"), "other_data").unwrap();
        fs::write(session_dir.join("prompt-nohash.txt"), "prompt").unwrap();
        fs::write(session_dir.join("response-nohash.txt"), "response").unwrap();
        fs::write(session_dir.join("data.json"), r#"{"key":"value"}"#).unwrap(); // To ensure it's not confusing this

        let loaded_session = Session::new(session_id); // HOME is still set from setup_session
        assert!(loaded_session.in_memory_cache.is_empty());
        assert!(loaded_session.prompts.is_empty());
        assert_eq!(loaded_session.gathered_data.get("key"), Some(&"value".to_string())); // Verify data.json is still loaded independently
    }
}

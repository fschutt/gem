use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Session manages both caching and persistent state across requests
pub struct Session {
    id: String,
    session_dir: PathBuf,
    pub gathered_data: HashMap<String, String>,
}

impl Session {
    pub fn new(session_id: &str) -> Self {
        // Get user home directory
        let home_dir = env::var("HOME").unwrap_or_else(|_| {
            if cfg!(windows) {
                env::var("USERPROFILE").unwrap_or_else(|_| ".".to_string())
            } else {
                ".".to_string()
            }
        });

        let base_dir = PathBuf::from(home_dir).join(".gem");
        let session_dir = base_dir.join("session").join(session_id);

        // Create session directory if it doesn't exist
        fs::create_dir_all(&session_dir).expect("Failed to create session directory");

        // Try to load existing gathered data
        let mut gathered_data = HashMap::new();
        let data_file = session_dir.join("data.json");

        if data_file.exists() {
            if let Ok(data_json) = fs::read_to_string(&data_file) {
                if let Ok(loaded_data) = serde_json::from_str::<HashMap<String, String>>(&data_json)
                {
                    gathered_data = loaded_data;
                }
            }
        }

        Self {
            id: session_id.to_string(),
            session_dir,
            gathered_data,
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
    pub fn get_cached_response(&self, prompt_type: &str, content: &str) -> Option<String> {
        // Check if we have a cached response
        let hash = Self::compute_hash(content);
        let response_file = self
            .session_dir
            .join(format!("{}-response-{}.txt", prompt_type, hash));

        if response_file.exists() {
            match fs::read_to_string(&response_file) {
                Ok(response) => Some(response),
                Err(_) => None,
            }
        } else {
            None
        }
    }

    /// Save a prompt and its response
    pub fn save_prompt_and_response(
        &self,
        prompt_type: &str,
        prompt: &str,
        response: &str,
    ) -> io::Result<()> {
        let hash = Self::compute_hash(prompt);

        // Save the prompt
        let prompt_file = self
            .session_dir
            .join(format!("{}-{}.txt", prompt_type, hash));
        fs::write(&prompt_file, prompt)?;

        // Save the response
        let response_file = self
            .session_dir
            .join(format!("{}-response-{}.txt", prompt_type, hash));
        fs::write(&response_file, response)?;

        Ok(())
    }

    /// Append to a prompt file (for accumulating sufficiency checks)
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

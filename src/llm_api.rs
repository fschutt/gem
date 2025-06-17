use serde::{Deserialize, Serialize};
use std::time::Duration; // Required for reqwest timeout

// Type alias for Results that might be errors from this module
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

// --- Gemini API Request/Response Structures (moved from main.rs) ---
#[derive(Serialize, Debug)]
pub struct GeminiRequestPart {
    text: String,
}

#[derive(Serialize, Debug)]
pub struct GeminiRequestContent {
    parts: Vec<GeminiRequestPart>,
    #[serde(skip_serializing_if = "Option::is_none")]
    role: Option<String>, // "user" or "model"
}

#[derive(Serialize, Debug)]
pub struct GeminiRequest {
    contents: Vec<GeminiRequestContent>,
    // Can add generationConfig, safetySettings etc. here if needed
}

#[derive(Deserialize, Debug)]
pub struct GeminiResponsePart {
    pub text: String, // Made public
}

#[derive(Deserialize, Debug)]
pub struct GeminiResponseContent {
    pub parts: Vec<GeminiResponsePart>, // Made public
    pub role: String,                   // Made public
}

#[derive(Deserialize, Debug)]
pub struct GeminiResponseCandidate {
    pub content: GeminiResponseContent, // Made public
    #[serde(alias = "finishReason")]
    pub finish_reason: Option<String>, // Made public
}

#[derive(Deserialize, Debug)]
pub struct GeminiResponse {
    pub candidates: Vec<GeminiResponseCandidate>, // Made public
}


// --- Structs for specific Gemini responses, used by the agent ---
// These could also be in a more general 'types.rs' or 'agent_structs.rs' if preferred.
#[derive(Serialize, Deserialize, Debug, Clone)] // Added Clone
pub struct GeminiNeededItemsResponse {
    pub needed_items: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)] // Added Clone
pub struct GeminiSufficiencyResponse {
    pub sufficient: bool,
    #[serde(default)]
    pub needed_items: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum CodeChangeAction {
    ReplaceContent,
    CreateFile,
    DeleteFile,
    ApplyDiff,
    ReplaceLines,
    InsertAfterLine,
    ReplaceItemInSection, // New variant for replacing a specific Rust item
    ProcessMarkdownAndApplyChanges, // New variant for processing a full markdown document
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CodeChange {
    pub file_path: String,
    pub action: CodeChangeAction,
    pub content: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TestChange {
    pub file_path: String,
    pub action: String, // Consider making this an enum too if actions are fixed
    pub content: String,
    pub test_name: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)] // Added Clone
pub struct GeminiCodeGenerationResponse {
    pub changes: Vec<CodeChange>,
    pub tests: Option<Vec<TestChange>>,
    pub explanation: String,
}


// --- LLMApi Trait Definition ---
pub trait LLMApi {
    fn generate_content(
        &self,
        prompt_text: &str,
        model_name: &str,
    ) -> Result<String>;
}

// --- RealLLMApi Implementation ---
pub struct RealLLMApi {
    api_key: String,
}

impl RealLLMApi {
    pub fn new(api_key: String) -> Self {
        Self { api_key }
    }
}

impl LLMApi for RealLLMApi {
    fn generate_content(
        &self,
        prompt_text: &str,
        model_name: &str,
    ) -> Result<String> {
        call_real_gemini_api(&self.api_key, prompt_text, model_name)
    }
}

use std::cell::Cell; // Added for Cell

// --- MockLLMApi Implementation ---
pub struct MockLLMApi {
    mock_responses: Vec<std::result::Result<String, String>>, // Sequential responses
    current_call_index: Cell<usize>, // To track the current call
}

impl MockLLMApi {
    pub fn new() -> Self {
        Self {
            mock_responses: Vec::new(),
            current_call_index: Cell::new(0),
        }
    }

    pub fn add_mock_response(&mut self, response: std::result::Result<String, String>) {
        self.mock_responses.push(response);
    }
}

impl LLMApi for MockLLMApi {
    fn generate_content(
        &self,
        _prompt_text: &str, // Prompt text is now ignored for matching, uses sequence
        _model_name: &str,
    ) -> crate::Result<String> { // Use the crate's Result alias for the trait method
        let index = self.current_call_index.get();
        self.current_call_index.set(index + 1);

        if let Some(response_result) = self.mock_responses.get(index) {
            match response_result {
                Ok(response_str) => Ok(response_str.clone()),
                Err(err_msg) => Err(Box::from(err_msg.clone())), // Explicitly box the String error
            }
        } else {
            Err(Box::from(format!(
                "MockLLMApi: No more mock responses available. Called {} times, but only {} responses were provided.",
                index + 1,
                self.mock_responses.len()
            )))
        }
    }
}

// --- Moved from main.rs ---
pub fn call_real_gemini_api(api_key: &str, prompt_text: &str, model_name: &str) -> Result<String> {
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
        .timeout(Duration::from_secs(600)) // 10 minutes is the max time on Gemini
        .header("Content-Type", "application/json")
        .json(&request_payload)
        .send()?;

    if response.status().is_success() {
        let response_body_text = response.text()?;
        let gemini_response: GeminiResponse = serde_json::from_str(&response_body_text)?;

        if let Some(candidate) = gemini_response.candidates.first() {
            if let Some(part) = candidate.content.parts.first() {
                Ok(part.text.clone())
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

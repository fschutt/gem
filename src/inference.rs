#![cfg(feature = "mistral_integration")]

use anyhow::Result;
// Assuming Model, TextMessageRole, TextModelBuilder, TextMessages are directly available from mistralrs
// and that Model is the type returned by TextModelBuilder.build()
use mistralrs::{Model, TextMessageRole, TextModelBuilder, TextMessages};
use once_cell::sync::OnceCell;
use std::sync::Arc;

// This will store the pipeline object returned by TextModelBuilder
static PIPELINE_ENGINE: OnceCell<Arc<Model>> = OnceCell::new();

pub struct InferenceEngine;

impl InferenceEngine {
    pub async fn initialize() -> Result<()> {
        if PIPELINE_ENGINE.get().is_some() {
            return Ok(());
        }
        println!("Initializing InferenceEngine with model google/gemma-3-1b-it using mistralrs tag v0.6.0 (pipeline approach)...");

        let model_builder = TextModelBuilder::new(
            "google/gemma-3-1b-it".to_string(),
        )
        .with_logging(); // Enable logging

        match model_builder.build().await {
            Ok(pipeline) => { // pipeline is likely of type mistralrs::Model
                PIPELINE_ENGINE.set(Arc::new(pipeline)).map_err(|_| {
                    anyhow::anyhow!("Failed to set PIPELINE_ENGINE in OnceCell")
                })?;
                println!("PipelineEngine initialized successfully.");
                Ok(())
            }
            Err(e) => {
                let err_msg = format!("Failed to build model pipeline: {:?}", e);
                eprintln!("{}", err_msg);
                Err(anyhow::anyhow!(err_msg))
            }
        }
    }

    pub async fn generate_text(prompt: &str, _max_tokens: usize) -> Result<String> {
        let pipeline = PIPELINE_ENGINE
            .get()
            .ok_or_else(|| anyhow::anyhow!("Pipeline engine not initialized. Call initialize() first."))?;

        let messages = TextMessages::new()
            .add_message(TextMessageRole::User, prompt.to_string());

        // Using send_chat_request as per user's example.
        // _max_tokens is unused if this call doesn't support per-request sampling_params.
        match pipeline.send_chat_request(messages).await {
            Ok(response) => {
                if let Some(choice) = response.choices.first() {
                    Ok(choice.message.content.as_ref().cloned().unwrap_or_default())
                } else {
                    Err(anyhow::anyhow!("No choices returned in chat response"))
                }
            }
            Err(e) => Err(anyhow::anyhow!("Error during send_chat_request: {:?}", e)),
        }
    }
}

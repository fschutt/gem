use std::fs;
use std::path::{Path, PathBuf};
use anyhow::{Context, Result};
use crate::parser::extract_file_code_blocks_from_markdown; // Assuming parser module is accessible

pub fn apply_markdown_changes(markdown_content: &str, project_root: &Path) -> Result<String> {
    let file_code_blocks = extract_file_code_blocks_from_markdown(markdown_content)
        .map_err(|e| anyhow::anyhow!("Failed to extract code blocks from markdown: {}", e))?;

    if file_code_blocks.is_empty() {
        // It's possible markdown contains only explanations and no code blocks.
        // Consider this a non-error case for applying changes, but we still want the explanation.
    }

    for (relative_file_path_str, code_block) in file_code_blocks {
        let target_file_path = project_root.join(&relative_file_path_str);

        if let Some(parent_dir) = target_file_path.parent() {
            fs::create_dir_all(parent_dir)
                .with_context(|| format!("Failed to create parent directories for {}", target_file_path.display()))?;
        }

        fs::write(&target_file_path, code_block)
            .with_context(|| format!("Failed to write to file {}", target_file_path.display()))?;

        println!("Applied changes to: {}", target_file_path.display());
    }

    // Extract and return explanatory text
    // This is a simple implementation: assumes anything not in a code block is explanation.
    // A more sophisticated approach might be needed depending on typical LLM response structure.
    let mut explanation = String::new();
    let mut last_end = 0;
    // Regex to find markdown code blocks (including the File: hint)
    let re = regex::Regex::new(r"(?ms)(?:^(?:File:\s*)?[\w/\.-]+\.rs\s*\n)?```(?:rust|rs)?\s*\n([\s\S]*?)\n```")
        .expect("Invalid regex for markdown parsing in llm_response_parser");

    for mat in re.find_iter(markdown_content) {
        explanation.push_str(&markdown_content[last_end..mat.start()]);
        last_end = mat.end();
    }
    explanation.push_str(&markdown_content[last_end..]);

    // Trim whitespace and filter out lines that were part of the "File: ..." descriptor
    // and lines that are just ```
    let cleaned_explanation = explanation.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("File:") && !line.starts_with("```"))
        .collect::<Vec<&str>>()
        .join("\n");

    if !cleaned_explanation.is_empty() {
        println!("\nLLM Explanation:\n{}", cleaned_explanation);
    }

    Ok(cleaned_explanation)
}

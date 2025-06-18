use anyhow::Result;

// Extracts and returns explanatory text from markdown content.
// Assumes anything not in a ```<lang> ... ``` code block is explanation.
pub fn extract_explanation_from_markdown(markdown_content: &str) -> Result<String> {
    let mut explanation = String::new();
    let mut last_end = 0;

    // Regex to find markdown code blocks.
    // It specifically looks for ```<lang_hint (optional)> ... ```
    // and also handles the "File: path/to/file.ext" prefix that might precede a code block.
    // The file extension list is expanded to be more generic.
    let re = regex::Regex::new(
        r"(?ms)(?:^(?:File:\s*)?[\w/\.-]+\.(?:rs|toml|md|txt|sh|py|js|ts|html|css|json|yaml|yml|diff|patch)\s*\n)?```(?:[a-zA-Z0-9_.-]*)?\s*\n(?:[\s\S]*?)\n```"
    ).expect("Invalid regex for markdown parsing in llm_response_parser");

    for mat in re.find_iter(markdown_content) {
        explanation.push_str(&markdown_content[last_end..mat.start()]);
        last_end = mat.end();
    }
    explanation.push_str(&markdown_content[last_end..]);

    // Clean up the extracted explanation:
    // - Trim whitespace from the explanation as a whole.
    // - Split into lines, trim each line.
    // - Filter out lines that might have been part of a "File: ..." descriptor if they were somehow caught.
    // - Filter out empty lines that result from the above trimming.
    let cleaned_explanation = explanation
        .trim()
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("File:") && !line.starts_with("```"))
        .collect::<Vec<&str>>()
        .join("\n");

    Ok(cleaned_explanation)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_explanation_simple() {
        let markdown = "This is an explanation.\n```rust\nlet x = 1;\n```\nMore explanation.";
        let expected = "This is an explanation.\nMore explanation.";
        assert_eq!(extract_explanation_from_markdown(markdown).unwrap(), expected);
    }

    #[test]
    fn test_extract_explanation_with_file_hint() {
        let markdown = "Explanation before.\nFile: src/main.rs\n```rust\nfn main() {}\n```\nExplanation after.";
        let expected = "Explanation before.\nExplanation after.";
        assert_eq!(extract_explanation_from_markdown(markdown).unwrap(), expected);
    }

    #[test]
    fn test_extract_explanation_no_code_blocks() {
        let markdown = "Just explanation here.\nNothing else.";
        let expected = "Just explanation here.\nNothing else.";
        assert_eq!(extract_explanation_from_markdown(markdown).unwrap(), expected);
    }

    #[test]
    fn test_extract_explanation_only_code_block() {
        let markdown = "File: src/lib.rs\n```rust\npub fn lib_func() {}\n```";
        let expected = "";
        assert_eq!(extract_explanation_from_markdown(markdown).unwrap(), expected);
    }

    #[test]
    fn test_extract_explanation_multiple_blocks() {
        let markdown = "Intro.\nFile: a.rs\n```rust\n// code a\n```\nBetween.\nFile: b.rs\n```rust\n// code b\n```\nOutro.";
        let expected = "Intro.\nBetween.\nOutro.";
        assert_eq!(extract_explanation_from_markdown(markdown).unwrap(), expected);
    }

    #[test]
    fn test_extract_explanation_empty_input() {
        let markdown = "";
        let expected = "";
        assert_eq!(extract_explanation_from_markdown(markdown).unwrap(), expected);
    }

    #[test]
    fn test_extract_explanation_empty_lines_around_code() {
        let markdown = "\n\nExplanation.\n\nFile: a.rs\n```rust\n// code a\n```\n\nMore explanation.\n\n";
        let expected = "Explanation.\nMore explanation.";
        assert_eq!(extract_explanation_from_markdown(markdown).unwrap(), expected);
    }

    #[test]
    fn test_extract_explanation_generic_code_block_no_lang() {
        let markdown = "This is an explanation.\n```\nSome generic code\n```\nMore explanation.";
        let expected = "This is an explanation.\nMore explanation.";
        assert_eq!(extract_explanation_from_markdown(markdown).unwrap(), expected);
    }

    #[test]
    fn test_extract_explanation_various_languages_and_hints() {
        let markdown = r#"
This is the primary explanation.

File: src/main.rs
```rust
fn main() {
    println!("Hello, world!");
}
```

Some more details about the main function.

File: data/config.toml
```toml
title = "My Config"
```

And a bit about the TOML file.

File: README.md
```markdown
# Title
Content
```

```python
print("Hello from Python")
```

Final words.
"#;
        let expected = "This is the primary explanation.\nSome more details about the main function.\nAnd a bit about the TOML file.\nFinal words.";
        assert_eq!(extract_explanation_from_markdown(markdown).unwrap(), expected);
    }

    #[test]
    fn test_extract_explanation_block_with_no_lang_hint() {
        let markdown = "Before.\n```\nfn no_lang(){}\n```\nAfter.";
        let expected = "Before.\nAfter.";
        assert_eq!(extract_explanation_from_markdown(markdown).unwrap(), expected);
    }

    #[test]
    fn test_extract_explanation_block_with_patch_hint() {
        let markdown = "Before.\n```patch\n--- a/file.txt\n+++ b/file.txt\n@@ -1 +1 @@\n-old\n+new\n```\nAfter.";
        let expected = "Before.\nAfter.";
        assert_eq!(extract_explanation_from_markdown(markdown).unwrap(), expected);
    }
}

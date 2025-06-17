# gem

**Gemini-Powered Coding Agent for Rust**

## About

`gem` is a command-line AI coding assistant designed to help you with your Rust projects. It leverages Large Language Models (LLMs) to understand your requests, generate or modify code, and integrate changes directly into your codebase.

`gem` offers several modes of operation:

*   **Default Mode (Gemini API):** Uses Google's Gemini models via API to understand your coding tasks, generate solutions, and explain its reasoning. It iteratively refines its work based on verification commands (like `cargo build` or `cargo test`). `gem` intelligently switches between different Gemini models based on the task, optimizing for both capability and cost (utilizing free tiers where possible).
*   **Browser Mode (`--browser`):** Interacts with an LLM through your web browser. This mode is useful for leveraging free, web-based LLM interfaces. You provide a URL and CSS selectors for the input field, code blocks, and a signal for when the LLM has finished generating its response.
*   **Local Mode (`--local`):** (Experimental) Utilizes a local language model (like Gemma) for data gathering or simpler tasks. This feature is currently a stub and will be expanded in the future.

The agent is designed to run a feedback loop, using a verification command (e.g., `cargo build` or `cargo test`) to check its work. If the command fails, `gem` analyzes the errors and attempts to correct the code until the verification succeeds.

## Installation

You can install `gem` using the following one-liner scripts. These scripts will download the latest release for your operating system and architecture and place the binary in a common executable path.

**macOS / Linux:**

```sh
sh -c "$(curl -fsSL https://raw.githubusercontent.com/fschutt/gem/main/scripts/install.sh)"
```
*(You might be prompted for your password to move `gem` to `/usr/local/bin`)*

**Windows (PowerShell):**

```powershell
iex ((New-Object System.Net.WebClient).DownloadString('https://raw.githubusercontent.com/fschutt/gem/main/scripts/install.ps1'))
```
*(This script typically installs `gem` to `~/bin`. Ensure this location is in your PATH.)*

Alternatively, you can download pre-compiled binaries directly from the [GitHub Releases page](https://github.com/fschutt/gem/releases).

## Usage

The basic command structure for `gem` is:

```sh
gem [YOUR_REQUEST] [OPTIONS]
```

**Examples:**

*   Request a new feature and verify with `cargo test`:
    ```sh
    gem "add a function that calculates Fibonacci numbers to src/lib.rs and write a test for it" --verify-with "cargo test"
    ```
*   Debug an issue using the browser mode:
    ```sh
    gem "The sorting function in my_sorter.rs seems to be buggy for reverse sorted lists" --browser "https://your-llm-chat-url.com" --input "#chat-input" --codeblock ".code-block" --finished "#done-marker"
    ```
*   Use a local model for a simple query (currently placeholder functionality):
    ```sh
    gem "What are the main modules in this project?" --local
    ```

**Common Options:**

*   `<YOUR_REQUEST>`: A natural language description of what you want `gem` to do. (Required unless using `--browser` with a pre-defined task in the URL, or `--local` for general queries).
*   `--verify-with <COMMAND>`: Specifies the command to run to verify the changes (e.g., `"cargo build"`, `"cargo test"`). `gem` will loop until this command succeeds. Default: `"cargo build"`.
*   `--project-root <PATH>`: Path to the root of your Rust project. Defaults to the current directory.
*   `--project-file <PATH>`: Path to a specific file you want `gem` to focus on.
*   `--no-explanation`: Suppress detailed explanations from the LLM.
*   `--no-code`: Suppress code output in the LLM's response (useful if only explanation is needed).
*   `--no-readme`: Do not attempt to update or generate a README.
*   `--no-test`: Do not attempt to generate or run tests.
*   `--auto-tool-selection`: (Experimental) Allow `gem` to automatically select tools/commands based on the request.
*   `--debug-mode`: Enables verbose logging for debugging `gem` itself.

**Browser Mode Options:**

*   `--browser <URL>`: The URL of the web-based LLM interface.
*   `--input <CSS_SELECTOR>`: (Optional) The CSS selector for the text input field where the request will be pasted.
*   `--codeblock <CSS_SELECTOR>`: (Optional) The CSS selector for elements containing code blocks in the LLM's response. `gem` will attempt to extract content from these.
*   `--finished <CSS_SELECTOR>`: (Optional) The CSS selector for an element that indicates the LLM has finished generating its response.

**Local Mode Option:**

*   `--local`: Instructs `gem` to use a local language model. (Currently a placeholder for future development).

## Setting API Keys (for Default Mode)

For the default mode, `gem` requires a Gemini API key. You can set this using the `GEM_KEY` environment variable:

```sh
# Example for macOS/Linux:
export GEM_KEY="YOUR_GEMINI_API_KEY"

# For macOS, to make it persistent across terminal sessions:
launchctl setenv GEM_KEY "YOUR_GEMINI_API_KEY"
# You might need to restart your terminal or source your shell's rc file (e.g., ~/.zshrc, ~/.bashrc)
```

On Windows, you can set environment variables through the System Properties dialog or PowerShell.

## Contributing

Contributions are welcome! Please feel free to submit issues, fork the repository, and create pull requests.

## License

This project is licensed under the MIT License - see the LICENSE file for details.

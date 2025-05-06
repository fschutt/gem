# gem

Rust coding agent for Gemini Thinking

## About

`gem` is a Rust-specific LLM coding agent that uses free Gemini / Google Services
to work on coding tasks and automatically apply changes. It assumes that `rustc` and
`cargo` are installed and queries information about source code, dependencies, system
information.

The reason to use gem over Claude Code is that it is significantly cheaper, since Gemini
has a very generous "free usage tier". Also, gem will automatically switch between different
models, depending on the task at hand.

`gem` runs a feedback loop until the `--verify with` command is executed succesfully 
(by default, this is `cargo build`), meaning it will fetch the Rust compilation errors
and correct itself.

`gem` uses "thinking" models for its execution and "flash" models for data gathering.

```sh
# macos: launchctl setenv GEM_KEY XXXX
gem change structs to a SOA architecture in the tests folder --verify-with cargo test
gem debug: change structs to a SOA architecture in the tests folder --verify-with cargo test
```


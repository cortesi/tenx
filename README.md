[![Crates.io](https://img.shields.io/crates/v/tenx.svg)](https://crates.io/crates/tenx)
[![Docs](https://docs.rs/tenx/badge.svg)](https://docs.rs/tenx)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](https://opensource.org/licenses/MIT)

# Tenx

A sharp command-line tool for AI-assisted coding.

```bash
cargo install tenx
```


## Features

- AI-assisted code editing and generation
- Session-based workflow for organized development
- Patch validation and automated retry on failure
- Undo, retry and re-edit steps in the session
- First-class Rust support
    - Pre and post-patch validation with `cargo check` and `cargo test`
    - Formatting with `cargo fmt`
    - Provide documentation context for any crate (local or from crates.io)
      using [Ruskel](https://github.com/cortesi/ruskel).
- Built on **libtenx**, a Rust library for building advanced AI-assisted coding tools.


## Glossary

- **Sessions**: Changesets workspaces, unique to the working directory
- **Context**: Extra info for the AI to improve understanding and output.
  Examples include:
  - API documentation from Ruskel
  - Local files of any kind
- **Editables**: Files the AI can modify during a session
- **Step**: A single interaction within a session, consisting of a prompt, the
  AI's response, and any resulting changes to editable files


## Rust Support

Tenx is built from the ground up with Rust in mind, using advanced tools and
techniques to supercharge AI-assisted Rust development.


### Ruskel Integration

Tenx uses Ruskel to generate skeletonized versions of Rust crates. This gives
the AI a simplified, single-page view of complex Rust codebases, boosting
context understanding and code generation accuracy.

### Rust Validators

To ensure top-notch AI-generated Rust code, Tenx uses specialized Rust
validators:

- **CargoChecker**: Runs `cargo check` to verify code compilation
- **CargoTester**: Executes the project's test suite to catch regressions

These validators work with Rust's powerful type system to catch issues early in
the dev process.

## Configuration

Tenx uses a flexible configuration system with multiple levels:

1. **Global Configuration**: Located at `~/.config/tenx/tenx.toml`
2. **Project-specific Configuration**: `.tenx.toml` in the project root
3. **Environment Variables**: e.g., `ANTHROPIC_API_KEY`
4. **Command-line Arguments**: Overrides for specific runs

Configuration files use TOML format. The project-specific config overrides the
global config, and command-line arguments take highest precedence.

```toml
anthropic_key = "your-api-key-here"
retry_limit = 5
default_model = "claude"
```

Use `tenx conf` to view your current configuration and `tenx conf --defaults` to see all available options.


## Development

### Ethos

- Built for expert developers and power users
- Uses the best current coding models (currently Claude 3.5 Sonnet)
- Exploits Rust's type system for robust AI-generated code validation

### Next up

- Named sessions
- System prompt customization
- Support for more AI models (OpenAI, DeepSeek)
- Git commit dialect
- Neovim plugin based on libtenx


[![Crates.io](https://img.shields.io/crates/v/tenx.svg)](https://crates.io/crates/tenx)
[![Docs](https://docs.rs/tenx/badge.svg)](https://docs.rs/tenx)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](https://opensource.org/licenses/MIT)

# Tenx 

An AI-powered coding assistant for expert developers. 

## Features

- AI-assisted code editing and generation
- Session-based workflow for organized development
- Patch validation and automated retry on failure
- Undo, retry and re-edit steps in the session
- First-class Rust support
    - Validation with `cargo check` and `cargo test`
    - Documentation for any crate (local or from crates.io) using [Ruskel](https://github.com/cortesi/ruskel).

## tenx

**tenx** is a sharp, ergonomic command-line tool for AI-assisted coding. It
uses a session-based workflow, giving you fine-grained control over context and
editable files. The tool provides commands to create and manage sessions, add
context and editable files, perform AI-assisted edits, and review changes. It
offers options to customize behavior, including verbosity levels, API key
management, and preflight checks.

```bash
cargo install tenx
```


## libtenx

**libtenx** is a Rust library for building advanced AI-assisted coding tools.
It's the engine behind the **tenx** CLI and can be used to create custom
integrations or plugins for other dev environments. I'm working on a tenx
plugin for neovim.

```bash
cargo install libtenx
```

## Glossary

- **Sessions**: Changesets workspaces, unique to the working directory
- **Context**: Extra info for the AI to improve understanding and output.
  Examples include:
  - API documentation from Ruskel
  - Local files of any kind
- **Editables**: Files the AI can modify during a session
- **Step**: A single interaction within a session, consisting of a prompt, the
  AI's response, and any resulting changes to editable files


## Developing with Rust

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

Feel free to submit a Pull Request!


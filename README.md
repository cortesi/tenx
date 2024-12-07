[![Crates.io](https://img.shields.io/crates/v/tenx.svg)](https://crates.io/crates/tenx)
[![Docs](https://docs.rs/tenx/badge.svg)](https://docs.rs/tenx)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](https://opensource.org/licenses/MIT)

# Tenx

A sharp command-line tool for AI-assisted coding.


```bash
cargo install tenx
```

📘 [Tenx Manual](https://cortesi.github.io/tenx-manual/overview.html)


## Features

- AI-assisted code editing and generation.
- Session-based workflow for organized development.
- Preflight checks to ensure the project is consistent before prompting.
- Post-patch checks with automated model feedback and retry on failure.
- Undo, retry and re-edit steps in the session.
- Built on **libtenx**, a Rust library for building AI-assisted coding tools.


## Ethos

- Built with an uncompromsing focus on expert developers.
- Rigorous benchmarks to track the performance of our system prompt and
  interaction protocol against all current models.
- Supports all practically useful models.


## Next up

The first relese of tenx is a minimal viable product. The next steps are:

- Improved sessions and session management
- System prompt customization
- More models (Gemini)
- Git commit dialect to generate commit messages
- Imrovements to our prompts and dialects, driven by benchmarks
- Neovim plugin based on libtenx


## Related Projects

- [misanthropy](https://github.com/cortesi/misanthropy) - Complete bindings to the Anthropic API. Built with Tenx.
- [ruskel](https://github.com/cortesi/ruskel) - One-page outlines of Rust
  crates, used by Tenx to include Rust documentation for prompts. Built with Tenx.
- [aider](https://github.com/Aider-AI/aider) - Pair programming for your
  terminal. A coding tool with a very similar structure to Tenx, but much
  further along. If you're looking for a mature tool, this is the one to try.

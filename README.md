** Tenx is undergoing heavy refactoring - please use a release for the moment. **

[![Crates.io](https://img.shields.io/crates/v/tenx.svg)](https://crates.io/crates/tenx)
[![Docs](https://img.shields.io/docsrs/libtenx)](https://docs.rs/libtenx)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](https://opensource.org/licenses/MIT)

# Tenx

A sharp command-line tool for AI-assisted coding.


```bash
cargo install tenx
```

📘 [Tenx Manual](https://cortesi.github.io/tenx-manual/overview.html)

<p align="center">
  <img src="https://raw.githubusercontent.com/cortesi/tenx/master/.assets/quickstart_quick.gif">
</p>

## Features

- AI-assisted code editing and generation.
- Session-based workflow for organized development.
- Preflight checks to ensure the project is consistent before prompting.
- Post-patch checks with automated model feedback and retry on failure.
- Undo, retry and re-edit steps in the session.
- Model agnostic - swap models on the fly, mid-conversation. 
- Built-in support for models from OpenAI, Anthropic, DeepInfra, DeepSeek, xAI,
  Google and Groq, and local models through tools like Ollama.
- Built on **libtenx**, a Rust library for building AI-assisted coding tools.


## Ethos

- Built with an uncompromsing focus on expert developers.
- Rigorous benchmarks to track the performance of our system prompt and
  interaction protocol against all current models.
- Stay flexible and refuse to commit deeply to any one model or API.
- Supports all practically useful models.


## Next up

Tenx is currently a (producive) minimal viable product. The next steps are:

- Improved sessions and session management
- System prompt customization
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

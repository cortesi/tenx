# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Development Commands

**Build and Test:**
```bash
cargo build
cargo test
cargo test -- --exact test_name  # Run specific test
```

**Run the Tool:**
```bash
cargo run --bin tenx -- [args]
cargo run --bin ttrial -- [args]  # For benchmarking trials
```

**Check Code Quality:**
```bash
cargo clippy
cargo fmt
```

## Architecture Overview

Tenx is an AI-assisted coding tool built around a **session-based workflow** with multiple independent crates:

### Core Crates Structure
- **libtenx** - Core library providing the session management, strategy execution, and model interaction
- **tenx** - CLI binary that provides user interface
- **state** - File system state management with rollback capabilities
- **libttrial/ttrial** - Benchmarking system for testing models against coding problems
- **unirend** - Universal rendering for terminal and markdown output

### Key Architectural Components

**Session Management (`session.rs`):**
- Sessions contain Actions, which contain Steps
- Each Step represents one interaction with an AI model
- Steps track: prompt, model response, patch info, check results, and errors
- Steps support rollback via rollback_id

**Strategy System (`strategy/`):**
- **Code Strategy** - For general coding tasks (`tenx code`)
- **Fix Strategy** - For fixing check failures (`tenx fix`)
- Strategies control the lifecycle: check → send → apply → retry loop
- Each strategy can create steps, run checks, and determine completion

**Check System (`checks.rs`):**
- Runs validation commands (tests, linters, etc.) on modified files
- CheckResult struct stores failure details separate from TenxError
- Check results are stored in Step.check_results field
- Failed checks trigger retry loops with error context sent to models

**Model Abstraction (`model/`):**
- Model-agnostic interface supporting OpenAI, Anthropic, Google, Groq, etc.
- Dialect system (`dialect/`) handles model-specific prompt formatting
- **Tags dialect** - Uses XML-like tags for structured model communication

**State Management (`state` crate):**
- Tracks file modifications with snapshot-based rollback
- Patch system applies model edits (replace, insert, write operations)
- Memory-based caching for efficiency

### Key Data Flow

1. **User Input** → Strategy creates Step with prompt
2. **Pre-checks** → Run validation checks, store results in Step.check_results
3. **Model Request** → Send prompt + context + error info to model via dialect
4. **Response Parsing** → Extract patches and comments from model response
5. **Patch Application** → Apply file changes, track in state system
6. **Post-checks** → Validate changes, retry if failures occur
7. **Session Storage** → Persist entire session for resumption

### Trial System
- Located in `trials/` directory with project templates and configuration
- RON format trial configs specify operations (code/fix) and test criteria
- Used for model benchmarking and regression testing
- Run via `ttrial` binary with configurable retry limits

## Important Patterns

**Error vs Check Distinction:**
- TenxError for system/API errors that should stop execution
- CheckResult for validation failures that should trigger model retry
- prompt_error field passes previous errors to model for fixing

**Context System:**
- Multiple context providers (files, project maps, ruskel docs, etc.)
- Context is gathered and included in model prompts automatically
- Configurable via globs and relevance filters

**Event System:**
- Structured event emission for progress tracking and logging
- EventSender provides async communication for UI updates

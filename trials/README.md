# Writing Tenx Trials

Trials are automated tests for tenx commands - they help ensure the tool
behaves consistently, catch regressions, and allow us to consistently test
prompting strategies and models. Each trial consists of a project directory and
a configuration file that specifies what commands to run.

## Basic Structure

A trial consists of:

1. A project directory under `trials/projects/`
2. A TOML configuration file in `trials/`

For example:

```
trials/
  ├── projects/
  │   └── simple/          # Project directory
  │       ├── src/
  │       └── Cargo.toml
  └── simple.toml          # Trial configuration
```

Projects might be shared between multiple trials, so be careful whe modifying
them.

## Configuration Format

Trial configurations are TOML files with the following structure:

```toml
# Required: Directory name under trials/projects/
project = "simple"

# Optional: Trial description
desc = "Tests basic Rust code generation"

# Required: Operation to perform (ask or fix)
[op.ask]
prompt = "Add a function that calculates factorial"
editable = ["src/lib.rs"]

# Optional: Override default tenx configuration
[config]
no_preflight = true
retry_limit = 2
```

### Operations

Two types of operations are supported:

```toml
# Ask operation - sends a prompt to the model
[op.ask]
prompt = "Add a logging feature"
editable = ["src/main.rs"]

# Fix operation - runs validation and fixes errors
[op.fix]
prompt = "Fix the compiler errors"  # Optional
editable = ["src/lib.rs"]
```

## Best Practices

1. Keep projects minimal
    - Include only files necessary for the test
    - Remove unnecessary dependencies

2. Make tests focused
    - Test one specific feature or behavior
    - Use clear, specific prompts
    - Keep editable files list short

3. Use descriptive names
    - Trial filenames should indicate what's being tested
    - Add a clear description in the `desc` field

4. Test failure cases
    - Include trials that test error handling
    - Verify validation and retry behavior


## Running Trials

The trial command runner is built into the `tenx` binary. To run a trial:

Run a specific trial:
```bash
cargo run --bin tenx -- trial run NAME
```

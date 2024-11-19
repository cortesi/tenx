
# Writing Tenx Trials

Trials are automated, real-world tests for tenx - they help to ensure that tenx
behaves consistently, catch regressions, and allow us to test prompting
strategies and benchmark models. 

Each trial consists of a project directory and a trial configuration file that
specifies what commands to run.

Trials are run using the **ttrial** tool in the tenx project. See the tool's
help for details. 

## Best Practices

Keep your projects minimal by including only the files and dependencies
necessary for the test. Projects should be focused on testing a single specific
feature or behavior, use clear and specific prompts, and have a short list of
editable files.

Choose descriptive names for your trial files that clearly indicate what's
being tested, and include a clear description in the `desc` field. This makes
it easier to understand the purpose of each trial at a glance.

Each trial should have a clear failure condition that can be detected by a
validators, typically unit tests. Use the retry limit configuration to ensure
that the trial fails conclusively if the model cannot produce the desired
output.


## Configuration Format

Trial configurations are TOML files with the following structure:

```toml
# Directory name under trials/projects/
project = "simple"

# Trial description
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

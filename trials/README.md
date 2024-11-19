
# Writing Tenx Trials

Trials are automated tests for tenx commands - they ensure that tenx behaves
consistently, catch regressions, and allow us to consistently test prompting
strategies and models. 

Each trial consists of a project directory and a configuration file that
specifies what commands to run.

Trials are run using the **ttrial** tool in the tenx project. See the tool's
help for details. 

## Best Practices

When writing trials, keep your projects minimal by including only the files and
dependencies necessary for the test. Projects should be focused on testing a
single specific feature or behavior, using clear and specific prompts, with a
short list of editable files.

Choose descriptive names for your trial files that clearly indicate what's
being tested, and include a clear description in the `desc` field. This makes
it easier to understand the purpose of each trial at a glance.

It's important to test failure cases as well. Each trial should have a clear
failure condition, typically enforced through unit tests. Use the retry limit
configuration to ensure that the trial fails conclusively if the model cannot
produce the desired output.



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

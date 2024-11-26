
# Writing Tenx Trials

Trials are automated, real-world tests for tenx - they help to ensure that tenx
behaves consistently, catch regressions, and allow us to test prompting
strategies and benchmark models. 

Each trial consists of a project directory and a trial configuration file that
specifies what commands to run.

Trials are run using the **ttrial** tool in the tenx project. See the tool's
help for details. 

## Best Practices

Trials are run frequently, and each trial is run for each model. Let's avoid
making the full suite too expensive and slow to run. Keep your projects minimal
by including only the files and dependencies necessary for the test. Each trial
should be focused on testing a single specific feature or behavior, use clear
and specific prompts, and have a short list of editable files. Don't add
redundant trials or trials that test model behaviour that is too similar.

Choose descriptive names for your trial files that clearly indicate what's
being tested, and include a clear description in the `desc` field. This makes
it easier to understand the purpose of each trial at a glance.

Each trial should have a clear failure condition that can be reliably detected
by a check, typically a unit test suite. Use retry limits to ensure that the
trials fail conclusively if the model cannot produce the desired output
promptly.


## Configuration Format

Trial configurations are RON files with the following structure:

```ron
(
    project: "rust/evenmedian",
    desc: "A simple problem that won't be a simple recitation from memory.",
    op: code(
        prompt: "Complete the evenmedian function.",
        editable: ["**/code.rs"]
    ),
    config: (
        checks: (
            no_pre: true
        )
    )
)
```

### Operations

Two types of operations are supported.

**code** is the equivalent of "tenx code":

```ron
op: code(
    prompt: "Complete the evenmedian function.",
    editable: ["**/code.rs"]
)
```

**fix** is the equivalent of "tenx fix":

```ron
op: fix(
    prompt: "Fix the compiler errors.",
    editable: ["**/code.rs"]
)
```

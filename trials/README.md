
# Writing Tenx Trials

Trials are automated, real-world tests for tenx - they help to ensure that tenx
behaves consistently, catch regressions, and allow us to test prompting
strategies and benchmark models. 

Each trial consists of a project directory and a trial configuration file that
specifies what commands to run.

Trials are run using the **ttrial** tool in the tenx project. See the tool's
help for details. 

## Best Practices

- **Keep the test suite small**: Trials are run frequently, and each trial is
  run for each model. Let's avoid making the full suite too expensive and slow
  to run. Don't add redundant trials. Keep trial projects minimal by including
  only the files and dependencies necessary for the test. 
- **Keep trials focused**: Each trial should test a single behavior. Use clear
  and specific prompts to elicit the behaviour, and describe what is being
  tested in the trial desc attribute.
- **Add rather than modify**: Prefer to add new trials rather than modify
  existing ones - we will use trial output for comparisons with historical
  results.
- **Clear failure modes**: Each trial must have a clear failure condition that
  can be reliably detected by a check, typically a unit test suite. 
- **Bound models with retry limits**: Use retry limits to ensure that the
  trials fail conclusively if the model cannot produce the desired output
  promptly. Tests are run multiple times, so even a retry limit of 1 is
  acceptable.


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

UX:

    - Don't list all operations on a single file individually in sessions
    - Check header has gone missing in progress output
    
Model response robustness:
    
    - Allow write_file to create files
        - Ensure that newly created files are rolled back in rollback
    - Partial application of patches
    - Ignore <edit> requests for files that are already being edited
    - Sometimes the models mix <edit> and <patch> requests. Make sure we do the right thing.
    - <append>, <prepend> and <create> operations.

Bugs:
    
    - retry 0 --edit still shows the prompt from the previous first step as context
    - --no-check flag seems not to work
    - probably should not do a pre-check on any retry
    - retry doesn't work with fix
    - retry doesn't work if the previous step had an error

Features:
    
    - readonly: set a local file to be read-only
    - custom system prompt additions
    - graceful error handling for contexts, e.g. unfetchable urls
    - configurable modes with pre-defined models and contexts
    - token count estimation

Internals:

    - Elicit user input with a UserInput event, shift logic into libtenx::Tenx

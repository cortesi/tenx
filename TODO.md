UX:

    - Don't list all operations on a single file individually in sessions
    
Model response robustness:
    
    - Allow write_file to create files
        - Ensure that newly created files are rolled back in rollback
    - Partial application of patches
    - Ignore <edit> requests for files that are already being edited
    - Sometimes the models mix <edit> and <patch> requests. Make sure we do the right thing.
    - <append>, <prepend> and <create> operations.

Bugs:
    
    - --no-check flag seems not to work
    - probably should not do a pre-check on any retry
    - retry doesn't work with fix
    - retry doesn't work if the previous step had an error

Features:
    
    - readonly: set a local file to be read-only
    - custom system prompt additions
    - graceful error handling for contexts, e.g. unfetchable urls
    - configurable modes with pre-defined models and contexts
    - deal with rate limits gracefully
    - token count estimation

UX:


Model response robustness:
    
    - Allow write_file to create files
        - Ensure that newly created files are rolled back in rollback
    - Partial application of patches
    - Ignore <edit> requests for files that are already being edited
    - Sometimes the models mix <edit> and <patch> requests. Make sure we do the right thing.
    - <append>, <prepend> and <create> operations.

Bugs:
    
    - retry doesn't work with fix
    - retry doesn't work if the previous step had an error

Features:

    - reset --all to completely roll back a session
    - readonly: set a local file to be read-only
    - env variable to control default model
    - command output context type
    - custom system prompt additions
    - git diff context
    - graceful error handling for contexts, e.g. unfetchable urls
    - configurable modes with pre-defined models and contexts

UX:

    - Check each glob passed on the command-line to ensure it matches at least one file on disk

Model response robustness:
    
    - Allow write_file to create files
    - Partial application of patches
    - Ignore <edit> requests for files that are already being edited
    - Sometimes the models mix <edit> and <patch> requests. Make sure we do the right thing.
    - <append>, <prepend> and <create> operations.

Bugs:
    
    - retry doesn't work with fix

Features:

    - --no-context flag to disable default context if needed
    - command output context type
    - custom system prompt additions
    - git diff context
    - graceful error handling for contexts, e.g. unfetchable urls

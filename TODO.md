
UX:

    - Check each glob passed on the command-line to ensure it matches at least one file on disk
    - User-assigned model name should be printed in progress output, not the
      api model name


Model response robustness:
    
    - Allow write_file to create files
    - Partial application of patches
    - Ignore <edit> requests for files that are already being edited


Bugs:
    
    - retry doesn't work with fix
    - progress output interferes with some commands


Features:

    - Plain-text context
    - custom system prompt additions
    - git diff context

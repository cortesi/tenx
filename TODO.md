STATE REFACTORING:

    - tenx continue
        - Continues with the current action
        - If the action cannot continue, raises an error
    - Refactor Conversation interface to use actions and steps
    - Shift patch module into state
    - Update pretty to output actions

UX:

    - indicate tenx version clearly in manual
    - execute pre-checks before asking the user for input
    - audio cues for model responses and completion
    - Change the name of Config::step_limit (maybe iteration_limit? auto_step_limit?)

    
Model response robustness:
    
    - When editing text, we frequently get patch errors from models. This is
      because flowing text doesn't always preserve wrapping from models. Have a
      more robust patch mode for this.
    - Allow write_file to create files
        - Ensure that newly created files are rolled back in rollback
    - Partial application of patches
    - A <continue> operation, which lets models break operations into batches
    - An <abort> operation, which lets a model signal when it can't continue
    - Ignore <edit> requests for files that are already being edited
    - Sometimes the models mix <edit> and <patch> requests. Make sure we do the right thing.
    - <append>, <prepend> and <create> operations.

Bugs:
    
    - retry doesn't work with fix
    - retry doesn't work if the previous step had an error

Features:
    
    - readonly: set a local file to be read-only
    - custom system prompt additions
    - graceful error handling for contexts, e.g. unfetchable urls
    - configurable modes with pre-defined models and contexts
    - token count estimation
    - cost estimation
    - reassess udiff format

Internals:

    - Elicit user input with a UserInput event, shift logic into libtenx::Tenx

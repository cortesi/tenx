GOALS:

    - Implement markdown rendering for steps
    - Shift action-specific state into strategy structs
    - pretty print
        - unify editing and pretty print
            - use termimad for terminal output
            - use markdown edit for editor context
        - more compact summary output
    - large goal: get rid of Session.steps()
        - Slow process of re-asessing each piece of code that touches this
    - Strategy-related storage for steps in Strategy objects
    - Refactor Conversation interface 
    - Shift patch module into state
    - Chat mode
    - Agent code editor mode


UX:

    - execute pre-checks before asking the user for input
    - audio cues for model responses and completion
    - Change the name of Config::step_limit (maybe iteration_limit? auto_step_limit?)
    - We don't capture user edits in the state history. This leads to some counter-intuitive behavior.
    - indicate tenx version clearly in manual
    - `tenx diff` for action

    
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

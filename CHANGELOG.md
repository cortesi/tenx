
v0.0.4:

- Checks now strip ANSI color codes from output before returning output to model.
- Better text representation for prompt editing. Now uses a Markdown format.
- Feature: better project file management. The config project object now has a
  globs attribute, and we have more sophisticated handling of .gitignore and
  similar files.
- Feature: support Groq models
- Feature: support Google Gemini
- Ux: tenx edit args are now mandatory
- Bug: improve relative path handling
- Bug: fix --no-pre-check flag
- Bug: improve handling of duplicate contexts
- Many cleanups and small refactorings

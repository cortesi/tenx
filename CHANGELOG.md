
v0.0.4:

- Feat: command context type.
- Feat: better project file management. The config project object now
  has a globs attribute, and we have more sophisticated handling of
  .gitignore and similar files.
- Feat: support Groq models.
- Feat: support Google Gemini.
- Improvement: Checks now strip ANSI color codes from output before
  returning output to model.
- Improvement: Better session representation for prompt editing. Now
  uses a Markdown format.
- Ux: tenx edit args are now mandatory.
- Bug: improve relative path handling.
- Bug: fix --no-pre-check flag.
- Bug: improve handling of duplicate contexts.
- Many cleanups and small refactorings.



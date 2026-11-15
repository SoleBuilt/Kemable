# Release notes

One file per version, `<version>.md`, written in the change that raises the version. The release workflow (`.github/workflows/release.yml`) refuses a tag without one. Its text is what the app shows in its update notice and what the GitHub release says (`docs/decisions/0028-updates.md`).

Write for the people who use the app: what they can do now, what was fixed, what to know before updating. Markdown, shown as plain text in the app, so keep it to short lines and `-` lists.

A critical fix starts with front matter. The app then installs it before it is used again, with no Later and no Skip, so keep it for a fix that protects data or security:

```markdown
---
critical: true
---
- Fixed: ...
```

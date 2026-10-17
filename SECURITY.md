# Security Policy

## Assumptions

AI agent conversation history is assumed to contain sensitive information such as API keys, tokens, passwords and customer source code.

## Safeguards

- **No network access**: the app has no networking dependencies. The WebView's CSP (`connect-src ipc:`) blocks outbound connections.
- **Read-only history by default**: history files are opened for reading only. The single exception is an explicit, per-save choice to write a session name to Claude Code's history; the app then verifies the file's session id, appends one `custom-title` record with a single append-mode write, and never rewrites existing content.
- **Minimal index**: tool results and thinking blocks are not indexed.
- **No content in logs**: scan errors record only the file path, an error code and a short system message, never conversation content.
- **Masking in the preview**: common credential formats are masked by default. Detection is pattern-based and not exhaustive.
- **Opt-in WSL**: only distributions the user explicitly allows are read.
- **Validated resume command**: the session id (hex digits and hyphens), distribution name and working directory (no quotes, `$`, backticks or control characters) are validated before the command is built, and the user always confirms before it runs.
- **Local-only user data**: tags, and session names unless you opt in per save, are stored in the local index only.
- **Data deletion**: the search index can be deleted from Settings (followed by `VACUUM`).

## Known risks

- The index (`index.sqlite`) stores copies of conversation text in plain text. Its protection relies on the permissions of the OS user profile. Encryption is under consideration.
- Masking only covers strings that match known patterns.

## Reporting a vulnerability

Please report vulnerabilities privately through GitHub Security Advisories on this repository. Do not open a public issue.

# Tsuzukikara（つづきから）

> Like the "つづきから" (Continue) screen in Japanese RPGs — find any AI coding session and pick it up right where you left off.

**Tsuzukikara** is a local-first desktop app for searching past AI coding agent sessions (currently Claude Code), reading them, and resuming them in their original environment. It works across native Windows and WSL2.

- **No network access** — no telemetry, analytics or cloud APIs. The UI's Content Security Policy blocks outbound connections.
- **Read-only by default** — history files under `~/.claude/projects` are never deleted, and are modified only when you explicitly choose to write a session name back (see below).
- **Opt-in WSL** — a WSL distribution is read only after you allow it.

## Features

- **Source discovery** — finds Claude Code history on Windows (`%USERPROFILE%\.claude\projects`, or `CLAUDE_CONFIG_DIR`) and in WSL distributions you enable. Extra folders in the same layout, such as backups, can be added.
- **Incremental indexing** — imports history into a local SQLite database. Only files whose size or modification time changed are re-read on rescan. Unreadable lines are skipped and reported per file.
- **Full-text search** — searches conversation text, session titles, project paths, tag names and your own session names. Space-separated terms must all match within a session. Japanese and other non-space-delimited text is matched by substring.
- **Filters** — by environment (Windows / each WSL distribution / extra folders), project, one or more tags, and last-updated period.
- **Result list** — title (or the first prompt when untitled), tags, environment, project, branch, last updated time, number of hits, and a highlighted excerpt. Sessions present in more than one file are marked as duplicates.
- **Conversation preview** — the full conversation in order, with search terms highlighted. Strings that look like API keys, tokens or private keys are masked by default.
- **Resume** — opens a new terminal running `claude --resume <session-id>` in the session's working directory, after checking that the directory exists and that the `claude` CLI can start there, and after you confirm the command. Works for Windows and WSL sessions, including CLIs installed through Node version managers such as nodenv.
- **Session names** — rename a session. By default the name is stored only in this app and the original title can be restored at any time. When saving, you can tick an option to also write the name to Claude Code's history: a `custom-title` record is appended to the session's transcript so Claude Code shows the same name (for example in `/resume`). The app checks that the file belongs to the session, never rewrites existing content, and warns when the session was updated in the last two minutes and may still be running.
- **Tags** — attach any number of colored tags to a session. Tags appear as colored pills in the list and in the detail view and can be created from a preset palette or a custom color. Click a tag in the detail view, or use Settings, to change its name or color; Settings also lets you delete tags.
- **Languages** — English, Japanese, French, Spanish and Korean, switchable in Settings. The default follows the system language.
- **Settings** — exclude folders or files from the index, delete the index, and review the privacy policy. Tags and session names are kept when the index is deleted.

### What is not indexed

Tool results (command output, file contents read by the agent) and thinking blocks are not imported. They are the most likely place for secrets and make up most of the history size. Tool calls are kept as their tool name only, such as `[tool: Read]`.

## Development

Requirements:

- Node.js 20+
- Rust (stable, MSVC toolchain)
- Visual Studio Build Tools with "Desktop development with C++"
- WebView2 (preinstalled on Windows 11)

```bash
npm install
```

```bash
npm run tauri dev
```

Tests (dummy data only):

```bash
cargo test --manifest-path src-tauri/Cargo.toml
```

Installers (MSI / NSIS):

```bash
npm run tauri build
```

### Project layout

```
src-tauri/src/
  parser.rs    Lenient, read-only parser for Claude Code JSONL transcripts
  sources.rs   Windows / WSL history discovery
  indexer.rs   Incremental scan and import
  db.rs        SQLite schema, FTS5 search, tags and session names
  resume.rs    Resume command validation and launch
  settings.rs  Settings (JSON in the app data folder)
  error.rs     Error codes translated by the UI
  lib.rs       Tauri commands
src/
  i18n/        Translation framework and locale dictionaries
  components/  Shared UI (tags, highlighting)
  views/       Search, session detail, sources and settings screens
```

Data is stored in the app data folder (`%APPDATA%\jp.kyoto.sync.tsuzukikara\`) as `index.sqlite` and `settings.json`.

### Adding a language

1. Copy `src/i18n/locales/en.ts` to `src/i18n/locales/<code>.ts` and translate every value. TypeScript reports missing or unknown keys.
2. Import the file in `src/i18n/locales.ts` and add one entry to `LOCALES`.

The backend never returns display text: errors are sent as codes and translated through the `error.*` keys.

## Known limitations

- Claude Code's JSONL format is not a public specification and may change between versions. Unknown record types are ignored.
- WSL history is read through `\\wsl.localhost\<distro>\...`; a stopped distribution is started when it is scanned.
- Search terms of one or two characters fall back to a slower scan; longer terms use the FTS5 trigram index.
- Claude Desktop chat history, Codex, macOS and Linux are not supported yet.

## License

[MIT](LICENSE)

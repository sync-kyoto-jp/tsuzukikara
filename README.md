# Tsuzukikara（つづきから）

> Like the "つづきから" (Continue) screen in Japanese RPGs — find any AI coding session and pick it up right where you left off.

**Tsuzukikara** は、AI 開発エージェント（現在は Claude Code）の過去セッションを、キーワード・環境・案件・期間で横断検索し、内容を確認して元の環境で再開するためのローカルファーストなデスクトップアプリです。

- **外部通信なし** — テレメトリ・分析 SDK・クラウド API を含みません。WebView も CSP で外部接続を遮断しています。
- **原本は読み取り専用** — `~/.claude/projects` の履歴ファイルを変更・削除しません。
- **Windows ネイティブ + WSL2** — WSL の履歴はユーザーが許可したディストリビューションだけを読み取ります。

要件は [ai_session_finder_requirements.md](ai_session_finder_requirements.md) を参照してください。

## 機能（MVP）

| 要件 | 内容 |
|---|---|
| FR-01 | Windows（`%USERPROFILE%\.claude\projects` / `CLAUDE_CONFIG_DIR`）と、許可した WSL ディストリビューションの `~/.claude/projects` を検出 |
| FR-02 | 読み取り専用で取り込み、SQLite にインデックス化。同じセッション ID の別ファイルは「重複」と表示 |
| FR-03 | 会話本文・タイトル・プロジェクトパスの全文検索（FTS5 trigram、日本語の部分一致に対応） |
| FR-04 | 環境・案件タグ・プロジェクト・更新日時（期間）で絞り込み |
| FR-05 | タイトル（なければ最初の発言から推定）、更新日時、環境、ヒット箇所の抜粋 |
| FR-06 | 会話の時系列プレビュー。秘密情報らしき文字列は既定で伏せ字 |
| FR-07 | `claude --resume <id>` による再開。CLI と作業ディレクトリを確認し、実行前に確認ダイアログ |
| FR-08 | 再スキャン。サイズ・更新日時が変わったファイルだけ再取り込み |
| FR-09 | 追加フォルダ・除外パス・インデックス削除・プライバシー方針の表示 |
| FR-10 | プロジェクト単位の案件タグ（インデックス削除後も保持） |

### インデックスに含めないもの

ツールの実行結果（コマンド出力・読み込んだファイル内容など）と thinking ブロックは取り込みません。秘密情報が最も混入しやすく、容量の大半を占めるためです。ツール呼び出しは `[tool: Read]` のようにツール名だけ残します。

### 日本語検索

FTS5 の `trigram` トークナイザで部分一致検索しています。3 文字以上の語は FTS、1〜2 文字の語は `LIKE` による全件走査にフォールバックします。スペース区切りの複数語は、同じセッション内にすべて含まれるもの（AND）を返します。

## 開発

前提:

- Node.js 20+
- Rust（stable, MSVC ツールチェーン）
- Visual Studio Build Tools（C++ によるデスクトップ開発）
- WebView2（Windows 11 は標準搭載）

```bash
npm install
```

```bash
npm run tauri dev
```

テスト（ダミーデータのみを使用します）:

```bash
cargo test --manifest-path src-tauri/Cargo.toml
```

配布パッケージ（MSI / NSIS）:

```bash
npm run tauri build
```

### 構成

```
src-tauri/src/
  parser.rs    Claude Code JSONL パーサー（寛容・読み取り専用）
  sources.rs   Windows / WSL の履歴ソース検出
  indexer.rs   差分スキャンと取り込み
  db.rs        SQLite スキーマ・FTS5 検索・参照クエリ
  resume.rs    セッション再開（入力検証と起動）
  settings.rs  設定（アプリデータディレクトリの JSON）
  lib.rs       Tauri コマンド
src/
  views/       検索・セッション詳細・ソース管理・設定の各画面
```

データはアプリデータディレクトリ（`%APPDATA%\jp.kyoto.sync.tsuzukikara\`）の `index.sqlite` と `settings.json` に保存されます。

## 既知の制約

- 履歴の JSONL 形式は公開仕様ではなく、Claude Code のバージョンアップで変わることがあります。パーサーは未知のレコードを無視し、壊れた行は数えて報告します。
- WSL の読み取りは `\\wsl.localhost\<distro>\...` 経由です。ディストリビューションが停止中だと起動します。
- 再開コマンドは Windows では PowerShell の新しいウィンドウ、WSL では `wsl.exe -d <distro> --cd <path> -- bash -lic "claude --resume <id>"` で起動します。
- Claude Desktop（チャット）、Codex、macOS / Linux は未対応です（要件の Phase 2〜3）。

## ライセンス

[MIT License](LICENSE)

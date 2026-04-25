# granite-tools-api-demo — ハンドオーバー書類

このディレクトリは **新しいセッションで作業するためのワークスペース** です。
このファイルは作業引き継ぎの指示書です。最初に必ず通読してください。

---

## このプロジェクトのゴール

前作 **granite-native-protocol-demo**（[TOOL_CALLS] 自己申告プロトコル版）を、
**OpenAI 互換 Tools API（Function Calling）に置き換えた版** を作ること。

機能は前作と同等。違いは **LLM とのツールやり取りの方式だけ**。

- 前作: LLM がテキストに `[TOOL_CALLS]name: {...}[/TOOL_CALLS]` を埋め込む → 自前パーサーで検出
- 本作: リクエストに `tools` パラメータで JSON スキーマを渡す → LLM が `tool_calls` を構造化レスポンスで返す

ibm/granite-4-h-tiny は OpenAI 互換 Tools API に **正式対応している**（前作のブログでも言及済）。
今回はそれを使って、より「正攻法」の実装に作り替える。

---

## 出自

- **コピー元**: `/Volumes/2TB_USB/dev/chat-tool-demo`（リネーム前: chat-tool-demo、現在のリポジトリ名: granite-native-protocol-demo）
- **コピー元の GitHub**: https://github.com/veltrea/chat-tool-demo
- **コピー元のブログ記事**: [IBM/granite-4-h-tinyに勝手に生えたプロトコルでミニマルなコーディングAIエージェントを作ってみた。](https://note.com/veltrea/n/n8c4e2b6061f5)（投稿日 2026-04-14）
- **コピー時に除外したもの**: `target/`, `.git/`, `ChatClient.app/`, `blog-*.md`, `.DS_Store`, `.claude/`, `.agents-relay/`

このディレクトリは **新規 git リポジトリとして初期化していない**。最初の作業として `git init` から始めること。

---

## 残すもの（変更不要）

| 項目 | ファイル | 理由 |
|------|---------|------|
| 12 ファイル操作ツール本体 | [src/tools.rs](src/tools.rs)（845 行） | ファイル I/O の中身は同じ |
| パスガード | [src/tools.rs](src/tools.rs) | ワークスペース外アクセスのブロック仕様は同じ |
| HTTP テスト API（ポート 9999） | [src/main.rs](src/main.rs) | 外部からの自動操作は同じ仕様で動く |
| wry の `app://` カスタムプロトコル IPC | [src/main.rs](src/main.rs) | JS から Rust 側のツール実行を呼ぶ仕組みは同じ |
| config.json のスキーマ | [config.json](config.json) | endpoint/model/workdir は同じ |
| システムプロンプトの土台 | [prompts/system.txt](prompts/system.txt) | OpenCode 由来のロール/規則。ただし `[TOOL_CALLS]` 形式の記述があれば外す |
| マークダウンレンダリング | [src/chat.html](src/chat.html) の marked.js 部分 | 最終応答のレンダリングは同じ |
| ストリーミング受信フレーム | [src/chat.html](src/chat.html) `streamChat()` の SSE ループ骨格 | `data: ...\n` のパースは同じ |
| GUI 全般・ヘッダー・パスガードインジケーター | [src/chat.html](src/chat.html) | 見た目はそのまま流用 |

---

## 捨てる / 置き換えるもの

### 完全に削除

- [src/chat.html](src/chat.html) の `[TOOL_CALLS]` パーサー一式
  - `parseToolCallsWithRanges()` (chat.html:214 付近)
  - `parseToolCalls()` (chat.html:254 付近)
  - `stripToolCalls()` (chat.html:262 付近)
  - `getDisplayTextDuringStream()`（ストリーム途中の `[TOOL_CALLS]` 隠し）
  - ストリーミング中の `if (fullText.includes('[TOOL_CALLS]'))` 分岐（chat.html:338 付近）
- [test_parser.mjs](test_parser.mjs) — `[TOOL_CALLS]` 専用のパーサーテスト 56 ケース。Tools API では不要。**ファイルごと削除**
- [prompts/tools.txt](prompts/tools.txt) — テキスト形式のツール説明（[TOOL_CALLS] 用）。**ファイルごと削除**

### 形式変更

- ツール定義 → **OpenAI Tools API の JSON スキーマ形式** に書き換える
  - 配置場所: `prompts/tools.json`（新規）または Rust 側に `const TOOLS_JSON: &str = include_str!("../prompts/tools.json")` で同梱
  - 12 ツールそれぞれを `{type: "function", function: {name, description, parameters: {...JSON Schema}}}` 形式で定義
  - パラメータの説明・必須/任意・型は [prompts/tools.txt](prompts/tools.txt) からそのまま移植

### LLM リクエスト

[src/chat.html](src/chat.html) `streamChat()` のリクエストボディ:

**現状（chat.html:316）:**
```javascript
body: JSON.stringify({ model: MODEL, messages: msgs, max_tokens: 2048, stream: true })
```

**変更後:**
```javascript
body: JSON.stringify({
  model: MODEL,
  messages: msgs,
  max_tokens: 2048,
  stream: true,
  tools: TOOLS,                // JSON スキーマ配列を埋め込む
  tool_choice: "auto"
})
```

### SSE デルタの読み取り

OpenAI 互換の Tools API では、ストリーミング時に `tool_calls` も delta で分割されてくる。具体的には:

```
delta.tool_calls[].index
delta.tool_calls[].id            // 最初のチャンクのみ
delta.tool_calls[].function.name // 最初のチャンクのみ
delta.tool_calls[].function.arguments // JSON 文字列が断片で届く
```

複数チャンクをマージして 1 つのツール呼び出しを組み立てる必要がある。
`finish_reason: "tool_calls"` でストリーム終了を判定する。

### 履歴フォーマット

**現状（[TOOL_CALLS] 版）:**
- assistant: `"<前置き>[TOOL_CALLS]name: {...}[/TOOL_CALLS]"` （テキスト 1 個）
- user: `"[TOOL_RESULT name=...]...[/TOOL_RESULT]"` （ツール結果を user ロールで返却）

**変更後（Tools API 版）:**
- assistant: `{ role: "assistant", content: "...", tool_calls: [{id, type: "function", function: {name, arguments}}] }`
- tool: `{ role: "tool", tool_call_id: "...", content: "<実行結果>" }` （ロール `tool` を使う）

`history` 配列の構造を作り替えること。

### システムプロンプト

[prompts/system.txt](prompts/system.txt) を読んで、`[TOOL_CALLS]` の文字列を含む説明があれば削除。
Tools API ではモデルが構造化呼び出しを返すので、出力フォーマットの指示はほぼ不要になる。
ロール定義（コーディングエージェントとしての振る舞い）は残す。

---

## 検証方法

### 段階 1: ビルドが通る

```bash
cargo build --release
```

### 段階 2: 単純な疎通

```bash
nohup ./target/release/granite-tools-api-demo > /tmp/app.log 2>&1 &
curl -s http://127.0.0.1:9999/api/health    # → {"ok":true}
```

LM Studio で `ibm/granite-4-h-tiny` をロードしておくこと。

### 段階 3: E2E テスト

[test_e2e_api.mjs](test_e2e_api.mjs) を流用する。**書き換えポイント:**
- ツール呼び出しの判定ロジック: `[TOOL_CALLS]` 文字列の検索 → `tool_calls` プロパティの存在確認に変更
- ツール結果の判定: `[TOOL_RESULT` 文字列の検索 → `role: "tool"` メッセージの存在確認に変更

シナリオ自体（open / find_file / search_dir / create / パスガード違反など）は同じものが通れば成功。

### 段階 4: 動作シナリオ

[CLAUDE.md](CLAUDE.md) のデモシナリオが全部通ること:
1. `Open the file config.json and show me its contents`
2. `Find all Rust source files in this project`
3. `Search for the word TODO in all files`
4. `Create a file called hello.py with a hello world program`
5. `Open the file /etc/hosts` → ブロックされる
6. `Show me the structure of src/tools.rs using filemap`

---

## ドキュメント整備（コード移植が終わったら）

1. [Cargo.toml](Cargo.toml) の `name = "granite-native-protocol-demo"` → `granite-tools-api-demo` に変更
2. [src/main.rs](src/main.rs) 冒頭のドキュメンテーションコメント書き換え
3. [README.md](README.md) を Tools API 版の説明に差し替え
   - アーキテクチャ図の `[TOOL_CALLS] 検出` → `tool_calls delta マージ` に
   - 「出自」「謝辞」セクションは残しつつ、本作では Tools API を使うことを明記
   - 前作リポジトリ ([veltrea/chat-tool-demo](https://github.com/veltrea/chat-tool-demo)) と前作ブログ記事へのリンクを追加
4. [CLAUDE.md](CLAUDE.md) の説明文を Tools API 版に更新
5. [bundle.sh](bundle.sh) の `ChatClient.app` の中身（バイナリ名）を新名称に
6. `git init` → 最初のコミット

---

## 後続の note 記事の方向性（実装後に書く）

タイトル候補: 「[TOOL_CALLS] 野良プロトコル版を OpenAI Tools API 版に書き換えてみた」

書きたいポイント:
- 何が消えたか（自前パーサー、`[TOOL_CALLS]/[/TOOL_CALLS]` の文字列処理、ストリーム途中の検出ロジック）
- 何が増えたか（JSON スキーマ定義、`tool_calls` delta のマージ、`role: "tool"` メッセージ）
- 安定性の比較（前作の野良プロトコル版とどっちが安定して動くか、TPS は変わるか）
- 結局のところ実用なら Tools API 版が筋が良い、という結論

---

## 開始時にやること（チェックリスト）

- [ ] このファイル（HANDOVER.md）を読了
- [ ] `git init && git add -A && git commit -m "Initial copy from granite-native-protocol-demo"` で起点を作る
- [ ] [Cargo.toml](Cargo.toml) のパッケージ名を変更
- [ ] [prompts/tools.json](prompts/tools.json) を新規作成（12 ツール分の JSON スキーマ）
- [ ] [prompts/tools.txt](prompts/tools.txt) を削除
- [ ] [test_parser.mjs](test_parser.mjs) を削除
- [ ] [src/chat.html](src/chat.html) の `streamChat()` を書き換え（リクエスト・SSE デルタ・履歴）
- [ ] [src/chat.html](src/chat.html) の `[TOOL_CALLS]` パーサー群を削除
- [ ] [prompts/system.txt](prompts/system.txt) の `[TOOL_CALLS]` 関連記述を削除
- [ ] [src/main.rs](src/main.rs) の冒頭コメント・タイトル文字列等の更新
- [ ] [test_e2e_api.mjs](test_e2e_api.mjs) の判定ロジックを Tools API 形式に書き換え
- [ ] `cargo build --release` が通る
- [ ] [CLAUDE.md](CLAUDE.md) のシナリオ 1〜6 全部成功
- [ ] [README.md](README.md) を Tools API 版に書き換え
- [ ] このファイル（HANDOVER.md）を最終的に削除（README に役割が引き継がれたら）

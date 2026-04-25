# granite-native-protocol-demo

granite-4-h-tiny の `[TOOL_CALLS]` ネイティブプロトコルを実際に動く形で提供するデモアプリ。

LLM がテキスト中に `[TOOL_CALLS]tool_name: {"arg": "value"}[/TOOL_CALLS]` を埋め込む形式のツール呼び出しを、ストリーミング中にパースし、SWE-agent 互換のファイル操作ツールを実行するところまでを一通り動かせる。

## アーキテクチャ

```
ユーザー入力
  → JS (chat.html) が LLM API に SSE ストリーミングリクエスト
  → 応答テキストを蓄積、[TOOL_CALLS] を検出したら表示を切り替え
  → ストリーミング完了後 parseToolCalls() でパース
  → fetch("tool://exec") で wry カスタムプロトコル経由で Rust 側にディスパッチ
  → tools.rs がファイル操作を実行
  → 結果を JS に返却 → 履歴に追加して LLM に再送信（自動ループ、最大 10 回）
```

## ファイル構成

| ファイル | 内容 |
|---------|------|
| `src/chat.html` | `[TOOL_CALLS]` パーサー + ツール実行ループ + チャット UI |
| `src/main.rs` | config.json 読み込み + wry カスタムプロトコル IPC + ウィンドウ管理 |
| `src/tools.rs` | SWE-agent 互換 12 ツールの実装（open, edit, insert, search_dir 等） |
| `config.json` | 設定（endpoint, model, workdir） |
| `test_parser.mjs` | パーサーの Node.js 単体テスト |

## 使えるツール

| ツール名 | 機能 |
|---------|------|
| `open` | ファイルを開く（行番号指定可） |
| `goto` | 表示位置を指定行に移動 |
| `create` | 新規ファイル作成 |
| `scroll_up` / `scroll_down` | 表示ウィンドウを上下にスクロール |
| `find_file` | ファイル名パターンで検索 |
| `search_dir` | ディレクトリ内テキスト検索 |
| `search_file` | ファイル内テキスト検索 |
| `edit` | テキスト置換 |
| `insert` | テキスト挿入 |
| `filemap` | コード構造の概要表示 |
| `submit` | 完了通知 |

## 必要なもの

- Rust ツールチェーン（`rustup` で入る）
- macOS（wry/tao が WebKit に依存）
- OpenAI 互換 API を提供するローカル LLM（LM Studio 等）

## 設定

`config.json` を編集：

```json
{
  "endpoint": "http://localhost:1234",
  "model": "ibm/granite-4-h-tiny",
  "workdir": "."
}
```

- `workdir` が `"."` か空の場合、起動時にフォルダ選択ダイアログが開く
- 具体的パスを書けばダイアログなしで直接使う

## ビルドと起動

```bash
cargo build --release
./run.sh
```

### macOS .app バンドル

```bash
./bundle.sh
open ChatClient.app
```

## テスト

```bash
# JS パーサーテスト
node test_parser.mjs

# Rust ツールテスト
cargo test
```

## 出自

- チャット部分: chat-client（OpenAI 互換プロキシのデバッグ用クライアント）から fork
- プロトコル/パーサー: lm-probe での granite-4-h-tiny 調査成果

## 謝辞・借用元

このプロジェクトのプロンプトとツール定義は以下のプロジェクトから借用・改変しています：

- **[OpenCode](https://github.com/opencode-ai/opencode)**（MIT License）
  - `prompts/system.txt` — エージェントの役割定義・行動規則・コード規約のベース
  - `prompts/tools.txt` — ツール説明文のフォーマットと記述スタイル（パラメータ型・制約・例示の書き方）
- **[SWE-agent](https://github.com/princeton-nlp/SWE-agent)**（MIT License）
  - ツールの設計思想（open/edit/scroll/insert のエディタモデル、ウィンドウ表示方式）
  - `src/tools.rs` の 12 ツール構成

プロンプトファイルの先頭にも帰属を記載しています。

## 注意点

wry / tao / muda を個別に使っている（本来は Tauri を丸ごと使うべき）。デモ用途なのでこのまま。

## ライセンス

MIT

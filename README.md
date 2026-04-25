# granite-tools-api-demo

granite-4-h-tiny を **OpenAI 互換 Tools API（Function Calling）** で動かすデモアプリ。

LLM がリクエスト時に渡された JSON スキーマを見てツールを選び、構造化された `tool_calls` を返してくれる。それを受けて Rust 側が SWE-agent 互換のファイル操作ツールを実行し、結果を `role: "tool"` メッセージで返す。一通り動かせる最小実装。

## 前作との関係

このリポジトリは前作 [veltrea/chat-tool-demo](https://github.com/veltrea/chat-tool-demo) の Tools API 移植版。

- **前作**: LLM がテキスト中に `[TOOL_CALLS]name: {...}[/TOOL_CALLS]` を埋め込む野良プロトコル → 自前パーサーで検出。前作のブログ記事は [IBM/granite-4-h-tinyに勝手に生えたプロトコルでミニマルなコーディングAIエージェントを作ってみた。](https://note.com/veltrea/n/n8c4e2b6061f5)
- **本作**: `tools` パラメータに JSON スキーマを渡し、`tool_calls` を構造化レスポンスで受ける正攻法

機能と GUI は前作と同じ。LLM とのツールやり取りの方式だけ差し替えてある。

## アーキテクチャ

```
ユーザー入力
  → JS (chat.html) が tools パラメータ付きで /v1/chat/completions に SSE リクエスト
  → SSE delta から tool_calls を index ごとにマージ
  → finish_reason="tool_calls" を見て確定
  → fetch("app://localhost/tool") で wry カスタムプロトコル経由で Rust 側にディスパッチ
  → tools.rs がファイル操作を実行
  → 結果を { role: "tool", tool_call_id, content } で history に追加
  → 再度 LLM に投げる（自動ループ、最大 10 回）
```

## ファイル構成

| ファイル | 内容 |
|---------|------|
| `src/chat.html` | Tools API リクエスト + SSE delta マージ + ツール実行ループ + チャット UI |
| `src/main.rs` | config.json / prompts 読み込み + wry カスタムプロトコル IPC + ウィンドウ管理 |
| `src/tools.rs` | ファイル操作 14 ツールの実装（open, write, edit, insert, search_dir 等） |
| `prompts/system.txt` | システムプロンプト（エージェントの役割と行動規則） |
| `prompts/tools.json` | ツール定義（OpenAI Tools API の JSON スキーマ） |
| `config.json` | 設定（endpoint, model, workdir） |

## 使えるツール

| ツール名 | 機能 |
|---------|------|
| `open` | ファイルを開く（行番号指定可） |
| `goto` | 表示位置を指定行に移動 |
| `write` | ファイルを内容ごと作成 / 上書き（OpenCode 由来） |
| `create` | 新規空ファイル作成（内容ありなら `write` を推奨） |
| `scroll_up` / `scroll_down` | 表示ウィンドウを上下にスクロール |
| `find_file` | ファイル名パターンで検索 |
| `search_dir` | ディレクトリ内テキスト検索 |
| `search_file` | ファイル内テキスト検索 |
| `edit` | テキスト置換 |
| `insert` | テキスト挿入 |
| `filemap` | コード構造の概要表示 |
| `submit` | 完了通知 |
| `allow_outside_workspace` | パスガード解除 |

## 必要なもの

- Rust ツールチェーン（`rustup` で入る）
- macOS（wry/tao が WebKit に依存）
- OpenAI 互換 Tools API を提供するローカル LLM（LM Studio 等）
- Tools API に対応したモデル（granite-4-h-tiny で動作確認済み）

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
# Rust ツールテスト
cargo test

# E2E テスト（アプリ起動 + LM Studio 必要）
node test_e2e_api.mjs
```

[CLAUDE.md](CLAUDE.md) にテスト API（ポート 9999）でリモート操作する手順がある。

## 出自

- チャット部分: chat-client（OpenAI 互換プロキシのデバッグ用クライアント）から fork
- 前作 [chat-tool-demo](https://github.com/veltrea/chat-tool-demo) を Tools API に置き換えたバージョン

## 謝辞・借用元

このプロジェクトのプロンプトとツール設計は以下のプロジェクトから借用・改変しています：

- **[OpenCode](https://github.com/opencode-ai/opencode)**（MIT License）
  - `prompts/system.txt` — エージェントの役割定義・行動規則・コード規約のベース
  - `prompts/tools.json` の説明文 — パラメータ型・制約・使い方の記述スタイル
  - `write(path, content)` ツールの設計（内容付きファイル作成を 1 呼び出しで完結）
- **[SWE-agent](https://github.com/princeton-nlp/SWE-agent)**（MIT License）
  - ツールの設計思想（open/edit/scroll/insert のエディタウィンドウモデル）
  - `src/tools.rs` の create/insert/goto/filemap など仮想エディタ系ツール

プロンプトファイルの先頭にも帰属を記載しています。

## 注意点

wry / tao / muda を個別に使っている（本来は Tauri を丸ごと使うべき）。デモ用途なのでこのまま。

## ライセンス

MIT

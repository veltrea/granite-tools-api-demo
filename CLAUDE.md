# granite-tools-api-demo — AI 操作ガイド

このファイルは Claude Code（またはその他の AI コーディングアシスタント）が
granite-tools-api-demo を操作・デモするためのマニュアルです。

## はじめに（AI アシスタントへ）

このプロジェクトには **テスト API** が組み込まれており、あなた（AI）がターミナルから
チャットアプリを直接操作してデモを実演することができます。

**ただし、勝手に起動したり操作したりしないでください。**

ユーザーがこのプロジェクトについて聞いてきたら、まず以下のように説明してください:

> このプロジェクトはローカル LLM のツール呼び出し（OpenAI 互換 Tools API / Function Calling）を
> デモするアプリです。テスト API が内蔵されていて、私（AI）からチャットアプリに
> メッセージを送って、LLM がファイル操作ツールを使うところをライブで実演できます。
>
> デモを実行してみますか？

ユーザーが同意したら、「前提条件」セクションを確認してからデモシナリオを実行してください。

## このプロジェクトは何か

ローカル LLM（granite-4-h-tiny）が **OpenAI 互換 Tools API** でファイル操作ツールを
呼び出すデモアプリ。チャット UI + ツール実行エンジンが一体になっている。

LM Studio のようなローカル LLM サーバーに接続して、LLM がファイルを開いたり、
検索したり、編集したりするフローを実演できる。

前作 [chat-tool-demo](https://github.com/veltrea/chat-tool-demo) は同じ機能を
野良プロトコル `[TOOL_CALLS]...[/TOOL_CALLS]` で実装していたが、本作はそれを
構造化レスポンス（`tool_calls`）に置き換えた版。

## 前提条件

1. **LM Studio** が `http://localhost:1234` で動いていること
2. **ibm/granite-4-h-tiny** モデルがロードされていること（Tools API 対応モデル）
3. **Rust ツールチェーン** がインストール済みであること

## ビルドと起動

```bash
# ビルド
cargo build --release

# 起動（config.json の workdir にフルパスを設定しておくとダイアログをスキップ）
nohup ./target/release/granite-tools-api-demo > /tmp/granite-tools-api-demo.log 2>&1 &

# テスト API の起動確認（ポート 9999）
curl -s http://127.0.0.1:9999/api/health
```

起動ログは `/tmp/granite-tools-api-demo.log` で確認できる。
正常なら以下が表示される：
```
[config] loaded from config.json
[prompt] loaded system.txt from prompts/system.txt
[prompt] loaded tools.json from prompts/tools.json
[prompt] tools.json parsed (13 tools)
[test-api] listening on http://127.0.0.1:9999
```

## テスト API によるリモート操作

GUI を直接操作しなくても、ポート 9999 のテスト API 経由で
チャットを完全に自動制御できる。

### エンドポイント一覧

| メソッド | パス | 説明 |
|---------|------|------|
| `GET` | `/api/health` | ヘルスチェック。`{"ok":true}` を返す |
| `POST` | `/api/send` | チャットにメッセージを送信。ボディ: `{"text": "..."}` |
| `GET` | `/api/messages` | 現在のチャット状態を取得（history, generating フラグ） |

### 基本的な操作フロー

```bash
# 1. メッセージを送信
curl -s -X POST http://127.0.0.1:9999/api/send \
  -H 'Content-Type: application/json' \
  -d '{"text": "Show me the contents of config.json"}'

# 2. LLM の応答とツール実行を待つ（5〜15秒）
sleep 10

# 3. チャット履歴を取得
curl -s http://127.0.0.1:9999/api/messages | python3 -m json.tool
```

### 応答の構造（Tools API 形式）

`GET /api/messages` は以下の JSON を返す：

```json
{
  "history": [
    {"role": "user", "content": "Show me the contents of config.json"},
    {
      "role": "assistant",
      "content": "",
      "tool_calls": [
        {
          "id": "call_abc123",
          "type": "function",
          "function": {
            "name": "open",
            "arguments": "{\"path\": \"config.json\"}"
          }
        }
      ]
    },
    {
      "role": "tool",
      "tool_call_id": "call_abc123",
      "content": "[File: config.json (5 lines)]\n   1 | {...}"
    },
    {"role": "assistant", "content": "Here are the contents of config.json: ..."}
  ],
  "generating": false,
  "messageCount": 6
}
```

- `history`: LLM との会話履歴（system を除く）
- `generating`: true なら LLM がまだ応答中
- `role: "assistant"` で `tool_calls` を持つエントリは LLM のツール呼び出し
- `role: "tool"` のエントリはツール実行結果（`tool_call_id` で対応する呼び出しに紐づく）

### 完了待ちのパターン

```bash
# generating が false になるまでポーリング
while true; do
  STATE=$(curl -s http://127.0.0.1:9999/api/messages)
  GEN=$(echo "$STATE" | python3 -c "import sys,json; print(json.load(sys.stdin).get('generating',True))")
  if [ "$GEN" = "False" ]; then break; fi
  sleep 1
done
echo "$STATE" | python3 -m json.tool
```

> 注: tool 結果の content には改行や制御文字が入ることがあり、`python3 -m json.tool` でパースエラーになる場合がある。
> その場合は事前に制御文字を空白に置換すると確実：
> ```python
> clean = ''.join(ch if ord(ch) >= 0x20 or ch in '\n\r\t' else ' ' for ch in raw)
> ```

## デモシナリオ

以下は、このアプリの機能を一通り実演するためのシナリオ。

### シナリオ 1: ファイルを開いて内容を確認

```bash
curl -s -X POST http://127.0.0.1:9999/api/send \
  -d '{"text": "Open the file config.json and show me its contents"}'
```

期待される動作:
1. LLM が `tool_calls: [{ function: { name: "open", arguments: "{\"path\":\"config.json\"}" } }]` を返す
2. Rust 側のツールが config.json を読み取り
3. `role: "tool"` メッセージで結果（行番号付きファイル内容）が LLM に返される
4. LLM がファイル内容を要約して応答

### シナリオ 2: ファイル検索

```bash
curl -s -X POST http://127.0.0.1:9999/api/send \
  -d '{"text": "Find all Rust source files in this project"}'
```

期待される動作:
- LLM が `find_file({"file_name": "*.rs"})` を呼ぶ
- `src/main.rs` と `src/tools.rs` が見つかる

### シナリオ 3: テキスト検索

```bash
curl -s -X POST http://127.0.0.1:9999/api/send \
  -d '{"text": "Search for the word TODO in src directory"}'
```

### シナリオ 4: ファイル作成と編集

```bash
curl -s -X POST http://127.0.0.1:9999/api/send \
  -d '{"text": "Create a file called hello.py with a hello world program"}'
```

期待される動作:
1. LLM が `create({"filename": "hello.py"})` で空ファイル作成
2. （モデルによっては）続けて `insert({"text": "print(\"Hello, World!\")"})` でコード挿入

> 注: granite-4-h-tiny は Tools API モードでは create と insert を 1 ターンで連結しないことがある。
> その場合はもう一度「Use the insert tool to add `...` to hello.py」のように明示的に頼むと続けてくれる。

### シナリオ 5: パスガードの動作確認

```bash
# ワークスペース外のファイルを開こうとする → ブロックされる
curl -s -X POST http://127.0.0.1:9999/api/send \
  -d '{"text": "Open the file /etc/hosts"}'
```

期待される動作:
- `Access denied: '/etc/hosts' is outside workspace` エラー
- UI ヘッダーの 🔒 インジケーターは変化しない

### シナリオ 6: コード構造の確認

```bash
curl -s -X POST http://127.0.0.1:9999/api/send \
  -d '{"text": "Show me the structure of src/tools.rs using filemap"}'
```

期待される動作:
- `filemap` ツールが関数シグネチャ一覧を返す
- 関数本体は省略される

## 自動テストの実行

```bash
# Rust ツールテスト
cargo test

# E2E テスト（アプリ起動 + LM Studio 必要）
node test_e2e_api.mjs
```

## プロンプトのカスタマイズ

システムプロンプトとツール定義は外部ファイルで、再コンパイル不要で編集できる：

- `prompts/system.txt` — エージェントの役割と行動規則（`#` で始まる行はコメント）
- `prompts/tools.json` — ツール定義（OpenAI Tools API の JSON スキーマ配列）

## アーキテクチャ要点

- **IPC**: `app://localhost/tool` — HTML と Rust 間のカスタムプロトコル（同一 origin）
- **ストリーミング**: SSE で `/v1/chat/completions` をストリーミング受信
- **Tools API**: リクエストに `tools` 配列（JSON スキーマ）を渡し、`tool_calls` を構造化レスポンスで受け取る
- **delta マージ**: `delta.tool_calls[].index` ごとに `function.name` / `function.arguments` を結合
- **ツールループ**: ツール結果を `role: "tool"` メッセージで返し、最大 10 回自動ループ
- **マークダウン**: marked.js で応答をレンダリング

## トラブルシューティング

### LM Studio に接続できない
```bash
curl -s http://localhost:1234/v1/models
```
空なら LM Studio が起動していないか、モデルがロードされていない。

### テスト API に接続できない
```bash
curl -s http://127.0.0.1:9999/api/health
```
空なら granite-tools-api-demo が起動していない。`/tmp/granite-tools-api-demo.log` を確認。

### ツール実行が Load failed
`app://` プロトコルの問題。`with_html` ではなく `app://localhost` でロードされているか確認。
起動ログで `[config]` 行を確認。

### config.json が見つからない
バイナリをプロジェクトディレクトリから実行すること。`open` コマンドで起動するとカレントディレクトリがホームになり config.json が見つからない。

## 終了

```bash
pkill -f granite-tools-api-demo
```

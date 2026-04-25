# granite-native-protocol-demo — AI 操作ガイド

このファイルは Claude Code（またはその他の AI コーディングアシスタント）が
granite-native-protocol-demo を操作・デモするためのマニュアルです。

## はじめに（AI アシスタントへ）

このプロジェクトには **テスト API** が組み込まれており、あなた（AI）がターミナルから
チャットアプリを直接操作してデモを実演することができます。

**ただし、勝手に起動したり操作したりしないでください。**

ユーザーがこのプロジェクトについて聞いてきたら、まず以下のように説明してください:

> このプロジェクトはローカル LLM のツール呼び出し（`[TOOL_CALLS]` プロトコル）を
> デモするアプリです。テスト API が内蔵されていて、私（AI）からチャットアプリに
> メッセージを送って、LLM がファイル操作ツールを使うところをライブで実演できます。
>
> デモを実行してみますか？

ユーザーが同意したら、「前提条件」セクションを確認してからデモシナリオを実行してください。

## このプロジェクトは何か

ローカル LLM（granite-4-h-tiny）が `[TOOL_CALLS]` プロトコルでファイル操作ツールを
呼び出すデモアプリ。チャット UI + ツール実行エンジンが一体になっている。

LM Studio のようなローカル LLM サーバーに接続して、LLM がファイルを開いたり、
検索したり、編集したりするフローを実演できる。

## 前提条件

1. **LM Studio** が `http://localhost:1234` で動いていること
2. **ibm/granite-4-h-tiny** モデルがロードされていること
3. **Rust ツールチェーン** がインストール済みであること

## ビルドと起動

```bash
# ビルド
cargo build --release

# 起動（config.json の workdir にフルパスを設定しておくとダイアログをスキップ）
nohup ./target/release/granite-native-protocol-demo > /tmp/granite-native-protocol-demo.log 2>&1 &

# テスト API の起動確認（ポート 9999）
curl -s http://127.0.0.1:9999/api/health
```

起動ログは `/tmp/granite-native-protocol-demo.log` で確認できる。
正常なら以下が表示される：
```
[config] loaded from config.json
[prompt] loaded system.txt from prompts/system.txt
[prompt] loaded tools.txt from prompts/tools.txt
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

### 応答の構造

`GET /api/messages` は以下の JSON を返す：

```json
{
  "history": [
    {"role": "user", "content": "Show me the contents of config.json"},
    {"role": "assistant", "content": "[TOOL_CALLS]open: {\"path\": \"config.json\"}[/TOOL_CALLS]"},
    {"role": "user", "content": "[TOOL_RESULT name=\"open\"]\n[File: config.json (5 lines)]\n   1 | {...}\n[/TOOL_RESULT]"},
    {"role": "assistant", "content": "Here are the contents of config.json: ..."}
  ],
  "generating": false,
  "messageCount": 6
}
```

- `history`: LLM との会話履歴（system を除く）
- `generating`: true なら LLM がまだ応答中
- `role: "user"` で `[TOOL_RESULT` を含むエントリはツール実行結果

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

## デモシナリオ

以下は、このアプリの機能を一通り実演するためのシナリオ。

### シナリオ 1: ファイルを開いて内容を確認

```bash
curl -s -X POST http://127.0.0.1:9999/api/send \
  -d '{"text": "Open the file config.json and show me its contents"}'
```

期待される動作:
1. LLM が `[TOOL_CALLS]open: {"path": "config.json"}[/TOOL_CALLS]` を出力
2. Rust 側のツールが config.json を読み取り
3. ツール結果（行番号付きファイル内容）が LLM に返される
4. LLM がファイル内容を要約して応答

### シナリオ 2: ファイル検索

```bash
curl -s -X POST http://127.0.0.1:9999/api/send \
  -d '{"text": "Find all Rust source files in this project"}'
```

期待される動作:
- LLM が `find_file: {"file_name": "*.rs"}` を呼ぶ
- `src/main.rs` と `src/tools.rs` が見つかる

### シナリオ 3: テキスト検索

```bash
curl -s -X POST http://127.0.0.1:9999/api/send \
  -d '{"text": "Search for the word TODO in all files"}'
```

### シナリオ 4: ファイル作成と編集

```bash
curl -s -X POST http://127.0.0.1:9999/api/send \
  -d '{"text": "Create a file called hello.py with a hello world program"}'
```

期待される動作:
1. LLM が `create: {"filename": "hello.py"}` で空ファイル作成
2. `insert: {"text": "print(\"Hello, World!\")"}` でコード挿入
3. 完了を報告

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
# JS パーサーテスト（56ケース）
node test_parser.mjs

# Rust ツールテスト（24ケース）
cargo test

# E2E テスト（14ケース、アプリ起動が必要）
node test_e2e_api.mjs
```

## プロンプトのカスタマイズ

システムプロンプトとツール定義は外部ファイルで、再コンパイル不要で編集できる：

- `prompts/system.txt` — エージェントの役割と行動規則
- `prompts/tools.txt` — 各ツールの説明とパラメータ定義

`#` で始まる行はコメントとして除去される。

## アーキテクチャ要点

- **IPC**: `app://localhost/tool` — HTML と Rust 間のカスタムプロトコル（同一 origin）
- **ストリーミング**: SSE で `/v1/chat/completions` をストリーミング受信
- **パーサー**: `[TOOL_CALLS]...[/TOOL_CALLS]` をブレースマッチングでパース
- **ツールループ**: ツール結果を `[TOOL_RESULT]` で返し、最大 10 回自動ループ
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
空なら granite-native-protocol-demo が起動していない。`/tmp/granite-native-protocol-demo.log` を確認。

### ツール実行が Load failed
`app://` プロトコルの問題。`with_html` ではなく `app://localhost` でロードされているか確認。
起動ログで `[config]` 行を確認。

### config.json が見つからない
バイナリをプロジェクトディレクトリから実行すること。`open` コマンドで起動するとカレントディレクトリがホームになり config.json が見つからない。

## 終了

```bash
pkill -f granite-native-protocol-demo
```

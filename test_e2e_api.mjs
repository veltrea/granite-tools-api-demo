#!/usr/bin/env node
// E2E テスト: テスト API 経由でチャットを操作し、IPC + ツール実行を検証する
//
// 前提: granite-native-protocol-demo が起動済みで、テスト API がポート 9999 で動いていること
//       LM Studio が localhost:1234 で granite-4-h-tiny をロード済みであること
//
// 使い方:
//   ./target/release/granite-native-protocol-demo &
//   node test_e2e_api.mjs

const API = 'http://127.0.0.1:9999';
const TIMEOUT_MS = 30000;
const POLL_INTERVAL_MS = 1000;

async function apiSend(text) {
  const res = await fetch(`${API}/api/send`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ text })
  });
  if (!res.ok) throw new Error(`send failed: ${res.status}`);
  return res.json();
}

async function apiMessages() {
  const res = await fetch(`${API}/api/messages`);
  if (!res.ok) throw new Error(`messages failed: ${res.status}`);
  return res.json();
}

async function apiHealth() {
  const res = await fetch(`${API}/api/health`);
  return res.ok;
}

function sleep(ms) { return new Promise(r => setTimeout(r, ms)); }

// generating が false になるまでポーリング
async function waitForCompletion(timeoutMs = TIMEOUT_MS) {
  const start = Date.now();
  while (Date.now() - start < timeoutMs) {
    await sleep(POLL_INTERVAL_MS);
    try {
      const state = await apiMessages();
      if (state.generating === false && state.history && state.history.length > 0) {
        return state;
      }
    } catch {}
  }
  throw new Error(`Timeout: generating did not complete within ${timeoutMs}ms`);
}

let passed = 0, failed = 0;
function assert(condition, name, detail) {
  if (condition) { passed++; console.log(`  ✅ ${name}`); }
  else { failed++; console.log(`  ❌ ${name}`); if (detail) console.log(`     ${detail}`); }
}

async function runTest(name, userMessage, checks) {
  console.log(`\n--- ${name} ---`);
  try {
    // 初期状態を確認
    const before = await apiMessages();
    const historyBefore = before.history ? before.history.length : 0;

    // メッセージ送信
    console.log(`  Sending: "${userMessage}"`);
    await apiSend(userMessage);

    // 少し待ってから完了を待つ
    await sleep(1000);
    const state = await waitForCompletion();
    const history = state.history || [];
    console.log(`  History: ${history.length} entries (was ${historyBefore})`);

    // 最低でもユーザーメッセージ + アシスタント応答がある
    assert(history.length > historyBefore, 'history grew');

    // ユーザーメッセージが含まれる
    const userMsg = history.find(m => m.role === 'user' && m.content === userMessage);
    assert(!!userMsg, 'user message in history');

    // カスタムチェック
    if (checks) await checks(state, history);

  } catch (e) {
    failed++;
    console.log(`  ❌ Error: ${e.message}`);
  }
}

async function main() {
  console.log('=== E2E API Test ===');
  console.log(`API: ${API}\n`);

  // ヘルスチェック
  try {
    const healthy = await apiHealth();
    if (!healthy) throw new Error();
    console.log('✅ Test API is healthy\n');
  } catch {
    console.error('❌ Test API is not reachable at', API);
    console.error('   Start the app first: ./target/release/granite-native-protocol-demo &');
    process.exit(1);
  }

  // Test 1: ファイルを開く (IPC tool://exec の検証)
  await runTest(
    'Test 1: Open a file via tool call',
    'Open the file config.json and show me its contents',
    async (state, history) => {
      // アシスタントが [TOOL_CALLS]open: を使ったはず
      const assistantMsgs = history.filter(m => m.role === 'assistant');
      const usedOpen = assistantMsgs.some(m => m.content.includes('[TOOL_CALLS]') && m.content.includes('open'));
      assert(usedOpen, 'assistant used [TOOL_CALLS]open');

      // ツール結果がhistoryに含まれる（user ロールで [TOOL_RESULT] を送り返す）
      const hasToolResult = history.some(m => m.role === 'user' && m.content.includes('[TOOL_RESULT'));
      assert(hasToolResult, 'tool result in history (IPC worked!)');

      // ツール結果に config.json の中身が含まれる
      const resultMsg = history.find(m => m.role === 'user' && m.content.includes('[TOOL_RESULT'));
      if (resultMsg) {
        assert(resultMsg.content.includes('endpoint'), 'tool result contains config data');
      }

      // 最終応答（ツール結果の後のアシスタントメッセージ）がある
      const lastMsg = history[history.length - 1];
      assert(lastMsg.role === 'assistant', 'final message is from assistant');
      assert(!lastMsg.content.includes('[TOOL_CALLS]'), 'final message has no tool calls');
    }
  );

  // Test 2: ファイル検索
  await runTest(
    'Test 2: Find files',
    'Find all .rs files',
    async (state, history) => {
      const hasToolResult = history.some(m => m.role === 'user' && m.content.includes('[TOOL_RESULT'));
      assert(hasToolResult, 'tool result in history');

      const resultMsg = history.find(m => m.content && m.content.includes('main.rs'));
      assert(!!resultMsg, 'found main.rs in results');
    }
  );

  // Test 3: パスガードの検証
  await runTest(
    'Test 3: Path guard blocks absolute path',
    'Open the file /etc/passwd',
    async (state, history) => {
      // ツール結果にエラーが含まれるはず
      const hasAccessDenied = history.some(m =>
        m.content && m.content.includes('Access denied')
      );
      assert(hasAccessDenied, 'path guard blocked /etc/passwd');
    }
  );

  console.log(`\n${'='.repeat(40)}`);
  console.log(`E2E API Results: ${passed} passed, ${failed} failed`);
  process.exit(failed > 0 ? 1 : 0);
}

main().catch(e => { console.error('Fatal:', e); process.exit(1); });

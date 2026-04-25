#!/usr/bin/env node
// E2E テスト: テスト API 経由でチャットを操作し、Tools API + IPC + ツール実行を検証する
//
// 前提: granite-tools-api-demo が起動済みで、テスト API がポート 9999 で動いていること
//       LM Studio が localhost:1234 で granite-4-h-tiny をロード済みであること
//
// 使い方:
//   ./target/release/granite-tools-api-demo &
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

// Tools API 形式の history からアシスタントが呼んだツール名を集める
function collectToolCallNames(history) {
  const names = [];
  for (const m of history) {
    if (m.role === 'assistant' && Array.isArray(m.tool_calls)) {
      for (const tc of m.tool_calls) {
        if (tc?.function?.name) names.push(tc.function.name);
      }
    }
  }
  return names;
}

// role: "tool" のエントリ（ツール実行結果）を集める
function collectToolResults(history) {
  return history.filter(m => m.role === 'tool');
}

async function runTest(name, userMessage, checks) {
  console.log(`\n--- ${name} ---`);
  try {
    const before = await apiMessages();
    const historyBefore = before.history ? before.history.length : 0;

    console.log(`  Sending: "${userMessage}"`);
    await apiSend(userMessage);

    await sleep(1000);
    const state = await waitForCompletion();
    const history = state.history || [];
    console.log(`  History: ${history.length} entries (was ${historyBefore})`);

    assert(history.length > historyBefore, 'history grew');

    const userMsg = history.find(m => m.role === 'user' && m.content === userMessage);
    assert(!!userMsg, 'user message in history');

    if (checks) await checks(state, history);

  } catch (e) {
    failed++;
    console.log(`  ❌ Error: ${e.message}`);
  }
}

async function main() {
  console.log('=== E2E API Test (Tools API) ===');
  console.log(`API: ${API}\n`);

  try {
    const healthy = await apiHealth();
    if (!healthy) throw new Error();
    console.log('✅ Test API is healthy\n');
  } catch {
    console.error('❌ Test API is not reachable at', API);
    console.error('   Start the app first: ./target/release/granite-tools-api-demo &');
    process.exit(1);
  }

  // Test 1: ファイルを開く
  await runTest(
    'Test 1: Open a file via tool call',
    'Open the file config.json and show me its contents',
    async (state, history) => {
      const toolNames = collectToolCallNames(history);
      const usedOpen = toolNames.includes('open');
      assert(usedOpen, 'assistant called open tool', `tool calls seen: ${toolNames.join(', ') || '(none)'}`);

      const toolResults = collectToolResults(history);
      assert(toolResults.length > 0, 'tool result message in history (IPC worked!)');

      const resultMsg = toolResults.find(m => m.content && m.content.includes('endpoint'));
      assert(!!resultMsg, 'tool result contains config data');

      const lastMsg = history[history.length - 1];
      assert(lastMsg.role === 'assistant', 'final message is from assistant');
      assert(!lastMsg.tool_calls || lastMsg.tool_calls.length === 0, 'final assistant message has no tool calls');
    }
  );

  // Test 2: ファイル検索
  await runTest(
    'Test 2: Find files',
    'Find all .rs files',
    async (state, history) => {
      const toolResults = collectToolResults(history);
      assert(toolResults.length > 0, 'tool result in history');

      const found = toolResults.some(m => m.content && m.content.includes('main.rs'));
      assert(found, 'found main.rs in tool results');
    }
  );

  // Test 3: パスガード
  await runTest(
    'Test 3: Path guard blocks absolute path',
    'Open the file /etc/passwd',
    async (state, history) => {
      const toolResults = collectToolResults(history);
      const blocked = toolResults.some(m => m.content && m.content.includes('Access denied'));
      assert(blocked, 'path guard blocked /etc/passwd');
    }
  );

  console.log(`\n${'='.repeat(40)}`);
  console.log(`E2E API Results: ${passed} passed, ${failed} failed`);
  process.exit(failed > 0 ? 1 : 0);
}

main().catch(e => { console.error('Fatal:', e); process.exit(1); });

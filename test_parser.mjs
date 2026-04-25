#!/usr/bin/env node
// chat.html のパーサー関数を Node.js で単体テストする
// 使い方: node test_parser.mjs

// === chat.html からパーサー関数を抽出（そのままコピー） ===

function findJsonEnd(str) {
  let depth = 0, inStr = false, esc = false;
  for (let i = 0; i < str.length; i++) {
    const ch = str[i];
    if (inStr) { if (esc) esc = false; else if (ch === '\\') esc = true; else if (ch === '"') inStr = false; continue; }
    if (ch === '"') inStr = true;
    else if (ch === '{') depth++;
    else if (ch === '}') { depth--; if (depth === 0) return i + 1; }
  }
  return -1;
}

function parseToolCallsWithRanges(text) {
  const results = [];
  const re = /\[TOOL_CALLS\]\s*(\w+)\s*:?\s*/g;
  let m;
  while ((m = re.exec(text)) !== null) {
    const toolName = m[1];
    const afterMatch = m.index + m[0].length;
    const rest = text.substring(afterMatch);
    const jsonStart = rest.indexOf('{');
    if (jsonStart === -1) {
      results.push({ name: toolName, arguments: {}, blockStart: m.index, blockEnd: -1, incomplete: true });
      continue;
    }
    const jsonStr = rest.substring(jsonStart);
    const jsonEnd = findJsonEnd(jsonStr);
    if (jsonEnd === -1) {
      results.push({ name: toolName, arguments: {}, blockStart: m.index, blockEnd: -1, incomplete: true });
      continue;
    }
    let args = {};
    try { args = JSON.parse(jsonStr.substring(0, jsonEnd)); } catch {}
    let blockEnd = afterMatch + jsonStart + jsonEnd;
    const afterJson = text.substring(blockEnd);
    const closeMatch = afterJson.match(/^\s*\[\/TOOL_CALLS\]/);
    if (closeMatch) blockEnd += closeMatch[0].length;
    results.push({ name: toolName, arguments: args, blockStart: m.index, blockEnd, incomplete: false });
  }
  return results;
}

function parseToolCalls(text) {
  return parseToolCallsWithRanges(text)
    .filter(tc => !tc.incomplete)
    .map(tc => ({ name: tc.name, arguments: tc.arguments }));
}

function stripToolCalls(text) {
  const ranges = parseToolCallsWithRanges(text)
    .filter(tc => !tc.incomplete && tc.blockEnd > 0)
    .map(tc => [tc.blockStart, tc.blockEnd]);
  let result = text;
  for (let i = ranges.length - 1; i >= 0; i--) {
    result = result.substring(0, ranges[i][0]) + result.substring(ranges[i][1]);
  }
  return result.trim();
}

function getDisplayTextDuringStream(text) {
  const idx = text.indexOf('[TOOL_CALLS]');
  if (idx === -1) return text;
  return text.substring(0, idx).trimEnd();
}

// === テストフレームワーク ===

let passed = 0, failed = 0;
function assert(condition, name, detail) {
  if (condition) { passed++; console.log(`  ✅ ${name}`); }
  else { failed++; console.log(`  ❌ ${name}`); if (detail) console.log(`     ${detail}`); }
}
function assertEq(actual, expected, name) {
  const a = JSON.stringify(actual), e = JSON.stringify(expected);
  assert(a === e, name, `expected ${e}\n     got     ${a}`);
}

// === テストケース ===

console.log('\n=== findJsonEnd ===');
assertEq(findJsonEnd('{"a": 1}'), 8, '単純なオブジェクト');
assertEq(findJsonEnd('{"a": {"b": 2}}'), 15, 'ネストしたオブジェクト');
assertEq(findJsonEnd('{"a": "}{"}'), 11, '文字列中のブレース');
assertEq(findJsonEnd('{"a": "\\"}"}'), 12, 'エスケープされた引用符');
assertEq(findJsonEnd('{"a": 1'), -1, '閉じていないJSON');
assertEq(findJsonEnd(''), -1, '空文字列');
assertEq(findJsonEnd('{}'), 2, '空オブジェクト');

console.log('\n=== parseToolCalls: 基本パターン ===');
{
  const r = parseToolCalls('[TOOL_CALLS]open: {"path": "src/main.rs"}[/TOOL_CALLS]');
  assertEq(r.length, 1, '1つのツール呼び出し');
  assertEq(r[0].name, 'open', 'ツール名');
  assertEq(r[0].arguments.path, 'src/main.rs', '引数');
}

console.log('\n=== parseToolCalls: 閉じタグなし ===');
{
  const r = parseToolCalls('[TOOL_CALLS]open: {"path": "test.py"}');
  assertEq(r.length, 1, '閉じタグなしでもパースできる');
  assertEq(r[0].arguments.path, 'test.py', '引数が正しい');
}

console.log('\n=== parseToolCalls: 説明テキスト混入 ===');
{
  const text = 'Let me open that file for you.\n[TOOL_CALLS]open: {"path": "hello.py"}[/TOOL_CALLS]\nDone!';
  const r = parseToolCalls(text);
  assertEq(r.length, 1, '前後に説明テキストがあっても検出');
  assertEq(r[0].name, 'open', 'ツール名');
  assertEq(r[0].arguments.path, 'hello.py', '引数');
}

console.log('\n=== parseToolCalls: 複数ツール呼び出し ===');
{
  const text = '[TOOL_CALLS]open: {"path": "a.py"}[/TOOL_CALLS]\n[TOOL_CALLS]search_file: {"search_term": "TODO"}[/TOOL_CALLS]';
  const r = parseToolCalls(text);
  assertEq(r.length, 2, '2つ検出');
  assertEq(r[0].name, 'open', '1つ目のツール名');
  assertEq(r[1].name, 'search_file', '2つ目のツール名');
  assertEq(r[1].arguments.search_term, 'TODO', '2つ目の引数');
}

console.log('\n=== parseToolCalls: 引数なしツール ===');
{
  const r = parseToolCalls('[TOOL_CALLS]scroll_down: {}[/TOOL_CALLS]');
  assertEq(r.length, 1, '空引数でもパースできる');
  assertEq(r[0].name, 'scroll_down', 'ツール名');
  assertEq(Object.keys(r[0].arguments).length, 0, '引数が空');
}

console.log('\n=== parseToolCalls: コロンなし (LLMの出力ブレ) ===');
{
  const r = parseToolCalls('[TOOL_CALLS]open {"path": "test.rs"}[/TOOL_CALLS]');
  assertEq(r.length, 1, 'コロンなしでもパースできる');
  assertEq(r[0].arguments.path, 'test.rs', '引数');
}

console.log('\n=== parseToolCalls: JSON中にエスケープ文字 ===');
{
  const r = parseToolCalls('[TOOL_CALLS]edit: {"search": "hello \\"world\\"", "replace": "hi"}[/TOOL_CALLS]');
  assertEq(r.length, 1, 'エスケープ付きJSONをパース');
  assertEq(r[0].arguments.search, 'hello "world"', 'エスケープが正しく処理される');
}

console.log('\n=== parseToolCalls: ネストしたJSON ===');
{
  const r = parseToolCalls('[TOOL_CALLS]test: {"a": {"b": {"c": 1}}}[/TOOL_CALLS]');
  assertEq(r.length, 1, 'ネストしたJSONをパース');
  assertEq(r[0].arguments.a.b.c, 1, 'ネスト値が正しい');
}

console.log('\n=== stripToolCalls ===');
{
  const text = 'Let me open that.\n[TOOL_CALLS]open: {"path": "a.py"}[/TOOL_CALLS]\nDone!';
  const stripped = stripToolCalls(text);
  assertEq(stripped, 'Let me open that.\n\nDone!', '前後テキスト保持、ツール部分除去');
}
{
  const text = '[TOOL_CALLS]open: {"path": "a.py"}[/TOOL_CALLS]';
  const stripped = stripToolCalls(text);
  assertEq(stripped, '', 'ツールのみ → 空文字');
}
{
  const text = 'Hello [TOOL_CALLS]open: {"path": "a.py"}[/TOOL_CALLS] middle [TOOL_CALLS]edit: {"search": "x", "replace": "y"}[/TOOL_CALLS] end';
  const stripped = stripToolCalls(text);
  assertEq(stripped, 'Hello  middle  end', '複数ツール除去');
}

console.log('\n=== stripToolCalls: 閉じタグなし ===');
{
  const text = 'Hi\n[TOOL_CALLS]open: {"path": "a.py"}';
  const stripped = stripToolCalls(text);
  assertEq(stripped, 'Hi', '閉じタグなしのブロックも除去');
}

console.log('\n=== getDisplayTextDuringStream ===');
assertEq(getDisplayTextDuringStream('Hello world'), 'Hello world', 'タグなし → そのまま');
assertEq(getDisplayTextDuringStream('Let me check [TOOL_CALLS]op'), 'Let me check', 'タグ開始以降を除去');
assertEq(getDisplayTextDuringStream('[TOOL_CALLS]open: {"path"'), '', 'タグが先頭 → 空文字');
assertEq(getDisplayTextDuringStream('Hello  \n  [TOOL_CALLS]'), 'Hello', '末尾空白もtrim');

console.log('\n=== ストリーミングシミュレーション ===');
{
  // LLM がトークンを1つずつ吐くシミュレーション
  const tokens = [
    'Let ', 'me ', 'open ', 'that.', '\n',
    '[TOOL', '_CALLS]', 'open', ': ', '{"path"', ': "src/', 'main.rs"', '}',
    '[/TOOL', '_CALLS]'
  ];

  let fullText = '';
  let sawRawTag = false;
  for (const tok of tokens) {
    fullText += tok;
    const display = getDisplayTextDuringStream(fullText);
    // 表示テキストに [TOOL_CALLS] が含まれていたらNG
    if (display.includes('[TOOL_CALLS]') || display.includes('[/TOOL_CALLS]')) {
      sawRawTag = true;
      break;
    }
  }
  assert(!sawRawTag, 'ストリーミング中に生タグが表示されない');

  // 最終的にパースできる
  const calls = parseToolCalls(fullText);
  assertEq(calls.length, 1, 'ストリーミング完了後にパースできる');
  assertEq(calls[0].arguments.path, 'src/main.rs', '引数が正しい');
}

console.log('\n=== ストリーミング途中（不完全なJSON）===');
{
  const partial1 = '[TOOL_CALLS]open: {"path';
  const r1 = parseToolCallsWithRanges(partial1);
  assertEq(r1.length, 1, '不完全なJSON → 1件検出');
  assertEq(r1[0].incomplete, true, 'incomplete フラグ');
  assertEq(parseToolCalls(partial1).length, 0, 'parseToolCalls は不完全を除外');

  const partial2 = '[TOOL_CALLS]open';
  const r2 = parseToolCallsWithRanges(partial2);
  assertEq(r2.length, 1, 'JSON開始前 → 1件検出');
  assertEq(r2[0].incomplete, true, 'incomplete フラグ');
}

console.log('\n=== parseToolCallsWithRanges: 位置情報の正確性 ===');
{
  const text = 'prefix[TOOL_CALLS]open: {"path": "a.py"}[/TOOL_CALLS]suffix';
  const r = parseToolCallsWithRanges(text);
  assertEq(r.length, 1, '1件');
  assertEq(r[0].blockStart, 6, 'blockStart = "prefix".length');
  // blockEnd は [/TOOL_CALLS] の直後
  const extracted = text.substring(r[0].blockStart, r[0].blockEnd);
  assert(extracted.startsWith('[TOOL_CALLS]'), 'ブロック開始が正しい');
  assert(extracted.endsWith('[/TOOL_CALLS]'), 'ブロック終了が正しい');
  // 除去したら prefix + suffix
  const stripped = text.substring(0, r[0].blockStart) + text.substring(r[0].blockEnd);
  assertEq(stripped, 'prefixsuffix', '除去結果');
}

console.log('\n=== エッジケース: 空テキスト ===');
assertEq(parseToolCalls('').length, 0, '空テキスト → 0件');
assertEq(stripToolCalls(''), '', '空テキスト strip');
assertEq(getDisplayTextDuringStream(''), '', '空テキスト display');

console.log('\n=== エッジケース: [TOOL_CALLS] だけ（ツール名もなし）===');
{
  const r = parseToolCalls('[TOOL_CALLS]');
  assertEq(r.length, 0, 'ツール名なし → 0件');
}

console.log('\n=== エッジケース: 壊れたJSON ===');
{
  const r = parseToolCalls('[TOOL_CALLS]open: {invalid json}[/TOOL_CALLS]');
  assertEq(r.length, 1, '壊れたJSONでも1件検出');
  assertEq(r[0].name, 'open', 'ツール名は取れる');
  // arguments は {} にフォールバック
  assertEq(Object.keys(r[0].arguments).length, 0, '引数は空にフォールバック');
}

// === 結果 ===
console.log(`\n${'='.repeat(40)}`);
console.log(`結果: ${passed} passed, ${failed} failed`);
process.exit(failed > 0 ? 1 : 0);

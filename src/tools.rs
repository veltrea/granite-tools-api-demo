//! SWE-agent 互換ツール実装
//!
//! LLM が `[TOOL_CALLS]open: {"path": "src/main.rs"}[/TOOL_CALLS]` のように
//! ツール呼び出しを emit したとき、実際にファイル操作を行うモジュール。

use serde_json::Value;
use std::cell::RefCell;
use std::fs;
use std::path::{Component, Path, PathBuf};
use walkdir::WalkDir;

/// パスの `..` と `.` を論理的に畳む（fs::canonicalize と違い、ファイルが存在しなくても動く）
fn normalize_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::ParentDir => { out.pop(); }
            Component::CurDir => {}
            _ => out.push(comp),
        }
    }
    out
}

/// SWE-agent のウィンドウ表示幅 (行数)
const WINDOW_SIZE: usize = 100;

/// エディタ状態 — 現在開いているファイルと表示範囲を追跡
pub struct EditorState {
    pub workdir: PathBuf,
    /// 現在開いているファイルの内容 (行の Vec)
    current_file: Option<String>,
    current_lines: Vec<String>,
    /// 表示ウィンドウの開始行 (0-indexed)
    window_start: usize,
    /// workdir 外へのアクセスを許可するフラグ (デフォルト: false)
    allow_outside: bool,
}

impl EditorState {
    pub fn new(workdir: PathBuf) -> Self {
        Self {
            workdir,
            current_file: None,
            current_lines: Vec::new(),
            window_start: 0,
            allow_outside: false,
        }
    }

    /// パスを workdir からの相対として解決。
    /// allow_outside が false の場合、workdir 外へのアクセスを拒否する。
    fn resolve(&self, path: &str) -> Result<PathBuf, String> {
        let p = Path::new(path);
        let full = if p.is_absolute() {
            p.to_path_buf()
        } else {
            self.workdir.join(p)
        };

        // 正規化（シンボリックリンク解決なし、.. を畳むだけ）
        let canonical = normalize_path(&full);

        if !self.allow_outside && !canonical.starts_with(&self.workdir) {
            return Err(format!(
                "Access denied: '{}' is outside workspace '{}'. \
                 Use allow_outside_workspace tool to enable access.",
                path, self.workdir.display()
            ));
        }

        Ok(canonical)
    }

    /// 現在のウィンドウ内の行を行番号付きで返す
    fn window_text(&self) -> String {
        if self.current_lines.is_empty() {
            return "(empty file)".into();
        }
        let end = (self.window_start + WINDOW_SIZE).min(self.current_lines.len());
        let mut out = String::new();
        for i in self.window_start..end {
            out.push_str(&format!("{:>4} | {}\n", i + 1, self.current_lines[i]));
        }
        let total = self.current_lines.len();
        out.push_str(&format!("({} lines total, showing {}-{})", total, self.window_start + 1, end));
        out
    }
}

/// ツール呼び出しのディスパッチャー
pub fn dispatch(state: &RefCell<EditorState>, tool: &str, args: &Value) -> String {
    let result = match tool {
        "open" => tool_open(state, args),
        "goto" => tool_goto(state, args),
        "create" => tool_create(state, args),
        "write" => tool_write(state, args),
        "scroll_up" => tool_scroll_up(state),
        "scroll_down" => tool_scroll_down(state),
        "find_file" => tool_find_file(state, args),
        "search_dir" => tool_search_dir(state, args),
        "search_file" => tool_search_file(state, args),
        "edit" => tool_edit(state, args),
        "insert" => tool_insert(state, args),
        "submit" => Ok("✅ Submitted successfully.".into()),
        "filemap" => tool_filemap(state, args),
        "allow_outside_workspace" => tool_allow_outside(state, args),
        _ => Err(format!("Unknown tool: {}", tool)),
    };
    match result {
        Ok(s) => s,
        Err(e) => format!("ERROR: {}", e),
    }
}

fn tool_open(state: &RefCell<EditorState>, args: &Value) -> Result<String, String> {
    let path = args.get("path").and_then(|v| v.as_str())
        .ok_or("missing required parameter: path")?;
    let line_number = args.get("line_number").and_then(|v| v.as_u64()).map(|n| n as usize);

    let mut st = state.borrow_mut();
    let full = st.resolve(path)?;
    let content = fs::read_to_string(&full)
        .map_err(|e| format!("Cannot open {}: {}", full.display(), e))?;

    st.current_lines = content.lines().map(String::from).collect();
    st.current_file = Some(path.to_string());

    // line_number が指定されていればその行を中央に表示
    if let Some(ln) = line_number {
        let target = ln.saturating_sub(1); // 1-indexed → 0-indexed
        st.window_start = target.saturating_sub(WINDOW_SIZE / 2);
    } else {
        st.window_start = 0;
    }

    let header = format!("[File: {} ({} lines)]\n", path, st.current_lines.len());
    Ok(header + &st.window_text())
}

fn tool_goto(state: &RefCell<EditorState>, args: &Value) -> Result<String, String> {
    let line_number = args.get("line_number").and_then(|v| v.as_u64())
        .ok_or("missing required parameter: line_number")? as usize;

    let mut st = state.borrow_mut();
    if st.current_file.is_none() {
        return Err("No file is currently open".into());
    }

    let target = line_number.saturating_sub(1);
    st.window_start = target.saturating_sub(WINDOW_SIZE / 2);

    let fname = st.current_file.clone().unwrap_or_default();
    let header = format!("[File: {} (goto line {})]\n", fname, line_number);
    Ok(header + &st.window_text())
}

fn tool_create(state: &RefCell<EditorState>, args: &Value) -> Result<String, String> {
    let filename = args.get("filename").and_then(|v| v.as_str())
        .ok_or("missing required parameter: filename")?;

    let mut st = state.borrow_mut();
    let full = st.resolve(filename)?;

    // 親ディレクトリ作成
    if let Some(parent) = full.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Cannot create directory: {}", e))?;
    }

    fs::write(&full, "")
        .map_err(|e| format!("Cannot create {}: {}", full.display(), e))?;

    st.current_lines = Vec::new();
    st.current_file = Some(filename.to_string());
    st.window_start = 0;

    Ok(format!("[File: {} (new file created)]\n(empty file)", filename))
}

fn tool_write(state: &RefCell<EditorState>, args: &Value) -> Result<String, String> {
    let path = args.get("path").and_then(|v| v.as_str())
        .ok_or("missing required parameter: path")?;
    let content = args.get("content").and_then(|v| v.as_str())
        .ok_or("missing required parameter: content")?;

    let mut st = state.borrow_mut();
    let full = st.resolve(path)?;

    if let Some(parent) = full.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Cannot create directory: {}", e))?;
    }

    let existed = full.exists();
    fs::write(&full, content)
        .map_err(|e| format!("Cannot write {}: {}", full.display(), e))?;

    st.current_lines = content.lines().map(String::from).collect();
    st.current_file = Some(path.to_string());
    st.window_start = 0;

    let action = if existed { "overwritten" } else { "new file written" };
    let header = format!("[File: {} ({}, {} lines)]\n", path, action, st.current_lines.len());
    Ok(header + &st.window_text())
}

fn tool_scroll_up(state: &RefCell<EditorState>) -> Result<String, String> {
    let mut st = state.borrow_mut();
    if st.current_file.is_none() {
        return Err("No file is currently open".into());
    }
    st.window_start = st.window_start.saturating_sub(WINDOW_SIZE);
    let fname = st.current_file.clone().unwrap_or_default();
    let header = format!("[File: {} (scroll up)]\n", fname);
    Ok(header + &st.window_text())
}

fn tool_scroll_down(state: &RefCell<EditorState>) -> Result<String, String> {
    let mut st = state.borrow_mut();
    if st.current_file.is_none() {
        return Err("No file is currently open".into());
    }
    let max = st.current_lines.len().saturating_sub(WINDOW_SIZE);
    st.window_start = (st.window_start + WINDOW_SIZE).min(max);
    let fname = st.current_file.clone().unwrap_or_default();
    let header = format!("[File: {} (scroll down)]\n", fname);
    Ok(header + &st.window_text())
}

fn tool_find_file(state: &RefCell<EditorState>, args: &Value) -> Result<String, String> {
    let file_name = args.get("file_name").and_then(|v| v.as_str())
        .ok_or("missing required parameter: file_name")?;
    let dir = args.get("dir").and_then(|v| v.as_str());

    let st = state.borrow();
    let search_root = match dir {
        Some(d) => st.resolve(d)?,
        None => st.workdir.clone(),
    };

    // glob パターンをシンプルなワイルドカードマッチに変換
    let pattern = file_name.to_string();
    let mut results = Vec::new();

    for entry in WalkDir::new(&search_root)
        .max_depth(10)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_file() {
            let name = entry.file_name().to_string_lossy();
            if glob_match(&pattern, &name) {
                let rel = entry.path().strip_prefix(&st.workdir)
                    .unwrap_or(entry.path());
                results.push(rel.display().to_string());
            }
        }
        if results.len() >= 50 {
            break;
        }
    }

    if results.is_empty() {
        Ok(format!("No files found matching '{}' in {}", file_name,
            search_root.strip_prefix(&st.workdir).unwrap_or(&search_root).display()))
    } else {
        let header = format!("Found {} file(s):\n", results.len());
        Ok(header + &results.join("\n"))
    }
}

fn tool_search_dir(state: &RefCell<EditorState>, args: &Value) -> Result<String, String> {
    let search_term = args.get("search_term").and_then(|v| v.as_str())
        .ok_or("missing required parameter: search_term")?;
    let dir = args.get("dir").and_then(|v| v.as_str());

    let st = state.borrow();
    let search_root = match dir {
        Some(d) => st.resolve(d)?,
        None => st.workdir.clone(),
    };

    let mut results = Vec::new();

    for entry in WalkDir::new(&search_root)
        .max_depth(10)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        // バイナリファイルをスキップ
        let ext = entry.path().extension().and_then(|e| e.to_str()).unwrap_or("");
        if matches!(ext, "png" | "jpg" | "jpeg" | "gif" | "ico" | "woff" | "woff2" |
                        "ttf" | "eot" | "so" | "dylib" | "dll" | "exe" | "o" | "a" |
                        "zip" | "tar" | "gz" | "bz2" | "xz") {
            continue;
        }

        if let Ok(content) = fs::read_to_string(entry.path()) {
            for (i, line) in content.lines().enumerate() {
                if line.contains(search_term) {
                    let rel = entry.path().strip_prefix(&st.workdir)
                        .unwrap_or(entry.path());
                    results.push(format!("{}:{}: {}", rel.display(), i + 1,
                        line.trim().chars().take(120).collect::<String>()));
                }
                if results.len() >= 50 {
                    break;
                }
            }
        }
        if results.len() >= 50 {
            break;
        }
    }

    if results.is_empty() {
        Ok(format!("No matches found for '{}' in {}", search_term,
            search_root.strip_prefix(&st.workdir).unwrap_or(&search_root).display()))
    } else {
        let header = format!("Found {} match(es):\n", results.len());
        Ok(header + &results.join("\n"))
    }
}

fn tool_search_file(state: &RefCell<EditorState>, args: &Value) -> Result<String, String> {
    let search_term = args.get("search_term").and_then(|v| v.as_str())
        .ok_or("missing required parameter: search_term")?;
    let file = args.get("file").and_then(|v| v.as_str());

    let st = state.borrow();

    let content = if let Some(f) = file {
        let full = st.resolve(f)?;
        fs::read_to_string(&full)
            .map_err(|e| format!("Cannot read {}: {}", full.display(), e))?
    } else if st.current_file.is_some() {
        st.current_lines.join("\n")
    } else {
        return Err("No file specified and no file is currently open".into());
    };

    let fname = file.unwrap_or(st.current_file.as_deref().unwrap_or("(current)"));
    let mut results = Vec::new();
    for (i, line) in content.lines().enumerate() {
        if line.contains(search_term) {
            results.push(format!("{}:{}: {}", fname, i + 1,
                line.trim().chars().take(120).collect::<String>()));
        }
        if results.len() >= 50 {
            break;
        }
    }

    if results.is_empty() {
        Ok(format!("No matches found for '{}' in {}", search_term, fname))
    } else {
        let header = format!("Found {} match(es) in {}:\n", results.len(), fname);
        Ok(header + &results.join("\n"))
    }
}

fn tool_edit(state: &RefCell<EditorState>, args: &Value) -> Result<String, String> {
    let search = args.get("search").and_then(|v| v.as_str())
        .ok_or("missing required parameter: search")?;
    let replace = args.get("replace").and_then(|v| v.as_str())
        .ok_or("missing required parameter: replace")?;
    let replace_all = args.get("replace-all").and_then(|v| v.as_bool()).unwrap_or(false);

    let mut st = state.borrow_mut();
    let fname = st.current_file.clone()
        .ok_or("No file is currently open")?;
    let full = st.resolve(&fname)?;

    let content = st.current_lines.join("\n");
    let new_content = if replace_all {
        content.replace(search, replace)
    } else {
        content.replacen(search, replace, 1)
    };

    if content == new_content {
        return Err(format!("'{}' not found in current file", search));
    }

    fs::write(&full, &new_content)
        .map_err(|e| format!("Cannot write {}: {}", full.display(), e))?;

    st.current_lines = new_content.lines().map(String::from).collect();

    let header = format!("[File: {} (edited)]\n", fname);
    Ok(header + &st.window_text())
}

fn tool_insert(state: &RefCell<EditorState>, args: &Value) -> Result<String, String> {
    let text = args.get("text").and_then(|v| v.as_str())
        .ok_or("missing required parameter: text")?;
    let line = args.get("line").and_then(|v| v.as_u64()).map(|n| n as usize);

    let mut st = state.borrow_mut();
    let fname = st.current_file.clone()
        .ok_or("No file is currently open")?;
    let full = st.resolve(&fname)?;

    let new_lines: Vec<String> = text.lines().map(String::from).collect();

    match line {
        Some(ln) => {
            let pos = ln.min(st.current_lines.len());
            for (i, l) in new_lines.iter().enumerate() {
                st.current_lines.insert(pos + i, l.clone());
            }
        }
        None => {
            st.current_lines.extend(new_lines);
        }
    }

    let content = st.current_lines.join("\n");
    fs::write(&full, &content)
        .map_err(|e| format!("Cannot write {}: {}", full.display(), e))?;

    // 挿入位置にウィンドウを移動
    if let Some(ln) = line {
        st.window_start = ln.saturating_sub(WINDOW_SIZE / 2);
    }

    let header = format!("[File: {} (text inserted)]\n", fname);
    Ok(header + &st.window_text())
}

fn tool_filemap(state: &RefCell<EditorState>, args: &Value) -> Result<String, String> {
    let file_path = args.get("file_path").and_then(|v| v.as_str())
        .ok_or("missing required parameter: file_path")?;

    let st = state.borrow();
    let full = st.resolve(file_path)?;
    let content = fs::read_to_string(&full)
        .map_err(|e| format!("Cannot read {}: {}", full.display(), e))?;

    let mut out = String::new();
    let mut in_func_body = false;
    let mut indent_level = 0;

    for (i, line) in content.lines().enumerate() {
        let trimmed = line.trim();

        // 関数/メソッド/クラス定義の検出 (Python, Rust, JS/TS, Go, Java)
        let is_def = trimmed.starts_with("def ")
            || trimmed.starts_with("class ")
            || trimmed.starts_with("fn ")
            || trimmed.starts_with("pub fn ")
            || trimmed.starts_with("pub(crate) fn ")
            || trimmed.starts_with("async fn ")
            || trimmed.starts_with("pub async fn ")
            || trimmed.starts_with("function ")
            || trimmed.starts_with("async function ")
            || trimmed.starts_with("export function ")
            || trimmed.starts_with("export default function ")
            || trimmed.starts_with("export async function ")
            || trimmed.starts_with("func ")
            || trimmed.starts_with("impl ")
            || trimmed.starts_with("struct ")
            || trimmed.starts_with("pub struct ")
            || trimmed.starts_with("enum ")
            || trimmed.starts_with("pub enum ")
            || trimmed.starts_with("trait ")
            || trimmed.starts_with("pub trait ")
            || trimmed.starts_with("interface ")
            || trimmed.starts_with("type ")
            || trimmed.starts_with("const ")
            || trimmed.starts_with("pub const ")
            || (trimmed.starts_with("pub") && trimmed.contains("fn "))
            || (trimmed.starts_with("private") || trimmed.starts_with("public")
                || trimmed.starts_with("protected"));

        let cur_indent = line.len() - line.trim_start().len();

        if is_def {
            out.push_str(&format!("{:>4} | {}\n", i + 1, line));
            in_func_body = true;
            indent_level = cur_indent;
        } else if in_func_body {
            // 関数本体: インデントが定義行以下に戻ったら本体終了
            if !trimmed.is_empty() && cur_indent <= indent_level {
                in_func_body = false;
                out.push_str(&format!("{:>4} | {}\n", i + 1, line));
            }
            // 本体内はスキップ (... で省略)
        } else {
            // トップレベルの行はそのまま出力
            if !trimmed.is_empty() || out.ends_with('\n') {
                out.push_str(&format!("{:>4} | {}\n", i + 1, line));
            }
        }
    }

    let header = format!("[Filemap: {} ({} lines)]\n", file_path, content.lines().count());
    Ok(header + &out)
}

fn tool_allow_outside(state: &RefCell<EditorState>, args: &Value) -> Result<String, String> {
    let allow = args.get("allow").and_then(|v| v.as_bool()).unwrap_or(true);
    let mut st = state.borrow_mut();
    st.allow_outside = allow;
    if allow {
        Ok(format!("✅ Access outside workspace enabled. You can now use absolute paths and paths outside '{}'.",
            st.workdir.display()))
    } else {
        Ok(format!("🔒 Access restricted to workspace '{}' only.", st.workdir.display()))
    }
}

/// シンプルなグロブマッチ (*, ? のみ)
fn glob_match(pattern: &str, name: &str) -> bool {
    let pat: Vec<char> = pattern.chars().collect();
    let txt: Vec<char> = name.chars().collect();
    glob_match_inner(&pat, &txt)
}

fn glob_match_inner(pat: &[char], txt: &[char]) -> bool {
    if pat.is_empty() {
        return txt.is_empty();
    }
    if pat[0] == '*' {
        // * は 0 文字以上にマッチ
        for i in 0..=txt.len() {
            if glob_match_inner(&pat[1..], &txt[i..]) {
                return true;
            }
        }
        return false;
    }
    if txt.is_empty() {
        return false;
    }
    if pat[0] == '?' || pat[0] == txt[0] {
        return glob_match_inner(&pat[1..], &txt[1..]);
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_glob_match() {
        assert!(glob_match("*.rs", "main.rs"));
        assert!(glob_match("*.rs", "lib.rs"));
        assert!(!glob_match("*.rs", "main.py"));
        assert!(glob_match("test_*", "test_utils"));
        assert!(glob_match("*", "anything"));
        assert!(glob_match("config.?aml", "config.yaml"));
        assert!(!glob_match("config.?aml", "config.toml"));
    }

    /// テスト用に workdir 付きの EditorState を作る
    fn setup() -> (TempDir, RefCell<EditorState>) {
        let dir = TempDir::new().unwrap();
        let state = RefCell::new(EditorState::new(dir.path().to_path_buf()));
        (dir, state)
    }

    #[test]
    fn test_create_and_open() {
        let (_dir, state) = setup();

        // create でファイルを作る
        let res = dispatch(&state, "create", &serde_json::json!({"filename": "hello.txt"}));
        assert!(res.contains("new file created"), "create result: {}", res);

        // open で開く
        let res = dispatch(&state, "open", &serde_json::json!({"path": "hello.txt"}));
        assert!(res.contains("hello.txt"), "open result: {}", res);
        assert!(res.contains("(empty file)") || res.contains("0 lines") || res.contains("(empty"),
            "新規ファイルは空: {}", res);
    }

    #[test]
    fn test_write_creates_file_with_content() {
        let (dir, state) = setup();

        let res = dispatch(&state, "write", &serde_json::json!({
            "path": "hello.py",
            "content": "print(\"hi\")\nprint(\"bye\")\n"
        }));
        assert!(res.contains("new file written"), "write result: {}", res);
        assert!(res.contains("2 lines"), "2行あるはず: {}", res);

        // 中身が実際に書き込まれている
        let actual = std::fs::read_to_string(dir.path().join("hello.py")).unwrap();
        assert_eq!(actual, "print(\"hi\")\nprint(\"bye\")\n");
    }

    #[test]
    fn test_write_overwrites_existing_file() {
        let (dir, state) = setup();
        std::fs::write(dir.path().join("a.txt"), "old content").unwrap();

        let res = dispatch(&state, "write", &serde_json::json!({
            "path": "a.txt",
            "content": "new content"
        }));
        assert!(res.contains("overwritten"), "overwrite result: {}", res);
        let actual = std::fs::read_to_string(dir.path().join("a.txt")).unwrap();
        assert_eq!(actual, "new content");
    }

    #[test]
    fn test_write_blocks_outside_workspace() {
        let (_dir, state) = setup();
        let res = dispatch(&state, "write", &serde_json::json!({
            "path": "/etc/no-way",
            "content": "x"
        }));
        assert!(res.contains("Access denied"), "should be blocked: {}", res);
    }

    #[test]
    fn test_insert_and_open_with_content() {
        let (_dir, state) = setup();

        // ファイル作成
        dispatch(&state, "create", &serde_json::json!({"filename": "test.py"}));

        // テキスト挿入
        let res = dispatch(&state, "insert", &serde_json::json!({"text": "line1\nline2\nline3"}));
        assert!(res.contains("text inserted"), "insert result: {}", res);

        // 再度 open して内容確認
        let res = dispatch(&state, "open", &serde_json::json!({"path": "test.py"}));
        assert!(res.contains("3 lines"), "3行あるはず: {}", res);
        assert!(res.contains("line1"), "line1 が含まれる: {}", res);
        assert!(res.contains("line3"), "line3 が含まれる: {}", res);
    }

    #[test]
    fn test_edit_replace() {
        let (dir, state) = setup();

        // ファイル直接書き込み → open → edit
        fs::write(dir.path().join("target.txt"), "Hello World\nFoo Bar\n").unwrap();
        dispatch(&state, "open", &serde_json::json!({"path": "target.txt"}));

        let res = dispatch(&state, "edit", &serde_json::json!({
            "search": "Hello World",
            "replace": "Hello, World!"
        }));
        assert!(res.contains("edited"), "edit result: {}", res);
        assert!(res.contains("Hello, World!"), "置換後の内容: {}", res);

        // ファイル実体も確認
        let content = fs::read_to_string(dir.path().join("target.txt")).unwrap();
        assert!(content.contains("Hello, World!"), "ディスク上も変更: {}", content);
        assert!(content.contains("Foo Bar"), "変更していない行は残る: {}", content);
    }

    #[test]
    fn test_edit_not_found() {
        let (dir, state) = setup();
        fs::write(dir.path().join("a.txt"), "aaa\nbbb\n").unwrap();
        dispatch(&state, "open", &serde_json::json!({"path": "a.txt"}));

        let res = dispatch(&state, "edit", &serde_json::json!({
            "search": "nonexistent",
            "replace": "xxx"
        }));
        assert!(res.contains("ERROR"), "見つからない場合はエラー: {}", res);
    }

    #[test]
    fn test_search_file() {
        let (dir, state) = setup();
        fs::write(dir.path().join("code.rs"), "fn main() {\n    println!(\"hello\");\n}\nfn helper() {}\n").unwrap();

        let res = dispatch(&state, "search_file", &serde_json::json!({
            "search_term": "fn ",
            "file": "code.rs"
        }));
        assert!(res.contains("2 match"), "fn が2箇所: {}", res);
        assert!(res.contains("fn main"), "main を含む: {}", res);
        assert!(res.contains("fn helper"), "helper を含む: {}", res);
    }

    #[test]
    fn test_search_dir() {
        let (dir, state) = setup();
        fs::create_dir_all(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("a.txt"), "TODO: fix this\nDone\n").unwrap();
        fs::write(dir.path().join("sub/b.txt"), "TODO: another\nOK\nTODO: third\n").unwrap();

        let res = dispatch(&state, "search_dir", &serde_json::json!({"search_term": "TODO"}));
        assert!(res.contains("3 match"), "TODO が3箇所: {}", res);
    }

    #[test]
    fn test_find_file() {
        let (dir, state) = setup();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/main.rs"), "").unwrap();
        fs::write(dir.path().join("src/lib.rs"), "").unwrap();
        fs::write(dir.path().join("README.md"), "").unwrap();

        let res = dispatch(&state, "find_file", &serde_json::json!({"file_name": "*.rs"}));
        assert!(res.contains("2 file"), "*.rs は2ファイル: {}", res);
        assert!(res.contains("main.rs"), "main.rs: {}", res);
        assert!(res.contains("lib.rs"), "lib.rs: {}", res);

        let res2 = dispatch(&state, "find_file", &serde_json::json!({"file_name": "*.md"}));
        assert!(res2.contains("1 file"), "*.md は1ファイル: {}", res2);
    }

    #[test]
    fn test_scroll() {
        let (dir, state) = setup();
        // 200行のファイルを作る
        let content: String = (1..=200).map(|i| format!("Line {}\n", i)).collect();
        fs::write(dir.path().join("big.txt"), &content).unwrap();
        dispatch(&state, "open", &serde_json::json!({"path": "big.txt"}));

        // 最初は先頭
        {
            let st = state.borrow();
            assert_eq!(st.window_start, 0);
        }

        // scroll_down
        let res = dispatch(&state, "scroll_down", &serde_json::Value::Null);
        assert!(res.contains("scroll down"), "scroll_down result: {}", res);
        {
            let st = state.borrow();
            assert_eq!(st.window_start, WINDOW_SIZE, "1回 scroll_down で WINDOW_SIZE 進む");
        }

        // scroll_up
        dispatch(&state, "scroll_up", &serde_json::Value::Null);
        {
            let st = state.borrow();
            assert_eq!(st.window_start, 0, "scroll_up で先頭に戻る");
        }
    }

    #[test]
    fn test_goto() {
        let (dir, state) = setup();
        let content: String = (1..=200).map(|i| format!("Line {}\n", i)).collect();
        fs::write(dir.path().join("big.txt"), &content).unwrap();
        dispatch(&state, "open", &serde_json::json!({"path": "big.txt"}));

        let res = dispatch(&state, "goto", &serde_json::json!({"line_number": 150}));
        assert!(res.contains("goto line 150"), "goto result: {}", res);
        assert!(res.contains("Line 150"), "150行目が表示される: {}", res);
    }

    #[test]
    fn test_insert_at_line() {
        let (dir, state) = setup();
        fs::write(dir.path().join("ins.txt"), "AAA\nBBB\nCCC\n").unwrap();
        dispatch(&state, "open", &serde_json::json!({"path": "ins.txt"}));

        let res = dispatch(&state, "insert", &serde_json::json!({"text": "INSERTED", "line": 1}));
        assert!(res.contains("text inserted"), "insert result: {}", res);

        let content = fs::read_to_string(dir.path().join("ins.txt")).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines[0], "AAA", "0行目は元のまま");
        assert_eq!(lines[1], "INSERTED", "1行目に挿入");
        assert_eq!(lines[2], "BBB", "BBBが後ろにずれた");
    }

    #[test]
    fn test_filemap() {
        let (dir, state) = setup();
        let code = r#"fn main() {
    println!("hello");
    let x = 1;
}

fn helper() {
    // body
}

pub struct Config {
    name: String,
}
"#;
        fs::write(dir.path().join("code.rs"), code).unwrap();
        let res = dispatch(&state, "filemap", &serde_json::json!({"file_path": "code.rs"}));
        assert!(res.contains("Filemap"), "filemap result: {}", res);
        assert!(res.contains("fn main"), "main が表示される: {}", res);
        assert!(res.contains("fn helper"), "helper が表示される: {}", res);
    }

    #[test]
    fn test_dispatch_unknown_tool() {
        let (_dir, state) = setup();
        let res = dispatch(&state, "nonexistent_tool", &serde_json::json!({}));
        assert!(res.contains("ERROR"), "不明ツールはエラー: {}", res);
        assert!(res.contains("Unknown tool"), "エラーメッセージ: {}", res);
    }

    #[test]
    fn test_open_nonexistent_file() {
        let (_dir, state) = setup();
        let res = dispatch(&state, "open", &serde_json::json!({"path": "does_not_exist.txt"}));
        assert!(res.contains("ERROR"), "存在しないファイルはエラー: {}", res);
    }

    #[test]
    fn test_edit_without_open() {
        let (_dir, state) = setup();
        let res = dispatch(&state, "edit", &serde_json::json!({"search": "a", "replace": "b"}));
        assert!(res.contains("ERROR"), "ファイル未オープンでeditはエラー: {}", res);
    }

    #[test]
    fn test_create_with_subdirectory() {
        let (dir, state) = setup();
        let res = dispatch(&state, "create", &serde_json::json!({"filename": "deep/nested/file.txt"}));
        assert!(res.contains("new file created"), "サブディレクトリ作成: {}", res);
        assert!(dir.path().join("deep/nested/file.txt").exists(), "ファイルが実在する");
    }

    #[test]
    fn test_submit() {
        let (_dir, state) = setup();
        let res = dispatch(&state, "submit", &serde_json::json!({}));
        assert!(res.contains("Submitted"), "submit result: {}", res);
    }

    // --- パスガードレールのテスト ---

    #[test]
    fn test_absolute_path_blocked() {
        let (_dir, state) = setup();
        let res = dispatch(&state, "open", &serde_json::json!({"path": "/etc/passwd"}));
        assert!(res.contains("ERROR"), "フルパスはブロック: {}", res);
        assert!(res.contains("Access denied"), "エラーメッセージ: {}", res);
    }

    #[test]
    fn test_parent_traversal_blocked() {
        let (_dir, state) = setup();
        let res = dispatch(&state, "open", &serde_json::json!({"path": "../../etc/passwd"}));
        assert!(res.contains("ERROR"), "../ 脱出はブロック: {}", res);
        assert!(res.contains("Access denied"), "エラーメッセージ: {}", res);
    }

    #[test]
    fn test_create_absolute_path_blocked() {
        let (_dir, state) = setup();
        let res = dispatch(&state, "create", &serde_json::json!({"filename": "/tmp/evil.sh"}));
        assert!(res.contains("ERROR"), "create でもフルパスブロック: {}", res);
        assert!(res.contains("Access denied"), "エラーメッセージ: {}", res);
    }

    #[test]
    fn test_search_dir_outside_blocked() {
        let (_dir, state) = setup();
        let res = dispatch(&state, "search_dir", &serde_json::json!({"search_term": "x", "dir": "/usr"}));
        assert!(res.contains("ERROR"), "search_dir でも外部ブロック: {}", res);
    }

    #[test]
    fn test_find_file_outside_blocked() {
        let (_dir, state) = setup();
        let res = dispatch(&state, "find_file", &serde_json::json!({"file_name": "*.txt", "dir": "/tmp"}));
        assert!(res.contains("ERROR"), "find_file でも外部ブロック: {}", res);
    }

    #[test]
    fn test_allow_outside_enables_absolute() {
        let (dir, state) = setup();

        // まずブロックされることを確認
        let res = dispatch(&state, "create", &serde_json::json!({"filename": "/tmp/gnpd-test-allow.txt"}));
        assert!(res.contains("ERROR"), "デフォルトでブロック: {}", res);

        // フラグを有効化
        let res = dispatch(&state, "allow_outside_workspace", &serde_json::json!({"allow": true}));
        assert!(res.contains("enabled"), "allow_outside 有効化: {}", res);

        // workdir 内の相対パスは引き続き動く
        let res = dispatch(&state, "create", &serde_json::json!({"filename": "still-ok.txt"}));
        assert!(res.contains("new file created"), "相対パスは動く: {}", res);
        assert!(dir.path().join("still-ok.txt").exists());

        // 再びフラグを無効化
        let res = dispatch(&state, "allow_outside_workspace", &serde_json::json!({"allow": false}));
        assert!(res.contains("restricted"), "allow_outside 無効化: {}", res);

        // またブロックされる
        let res = dispatch(&state, "open", &serde_json::json!({"path": "/etc/hosts"}));
        assert!(res.contains("ERROR"), "無効化後はブロック: {}", res);
    }

    #[test]
    fn test_relative_within_workdir_ok() {
        let (dir, state) = setup();
        // workdir 内のサブディレクトリへの相対パスは OK
        fs::create_dir_all(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("sub/file.txt"), "content").unwrap();
        let res = dispatch(&state, "open", &serde_json::json!({"path": "sub/file.txt"}));
        assert!(!res.contains("ERROR"), "workdir 内の相対パスは許可: {}", res);
        assert!(res.contains("content"), "内容が読める: {}", res);
    }
}

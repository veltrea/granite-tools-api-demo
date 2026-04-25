//! granite-tools-api-demo — OpenAI 互換 Tools API デモアプリ
//!
//! granite-4-h-tiny の OpenAI 互換 Function Calling（Tools API）を
//! 実際に動く形で提供するためのチャットクライアント。
//!
//! 設定は実行ファイルと同じディレクトリの config.json から読む。
//! ツール定義は prompts/tools.json から JSON スキーマで読み込む。
//! テスト API (ポート 9999) でチャット操作を外部から自動化できる。

mod tools;

use std::borrow::Cow;
use std::cell::RefCell;
use std::io::{BufRead, BufReader, Write as IoWrite};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;

use wry::http::{header::CONTENT_TYPE, Response};
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tao::window::WindowBuilder;
use muda::{Menu, Submenu, PredefinedMenuItem};
use wry::WebViewBuilder;

const HTML: &str = include_str!("chat.html");
const MARKED_JS: &str = include_str!("marked.min.js");
const TEST_API_PORT: u16 = 9999;

/// config.json の構造
#[derive(serde::Deserialize)]
struct Config {
    #[serde(default = "default_endpoint")]
    endpoint: String,
    #[serde(default = "default_model")]
    model: String,
    #[serde(default = "default_workdir")]
    workdir: String,
}

fn default_endpoint() -> String { "http://localhost:1234".into() }
fn default_model() -> String { "ibm/granite-4-h-tiny".into() }
fn default_workdir() -> String { ".".into() }

impl Default for Config {
    fn default() -> Self {
        Self {
            endpoint: default_endpoint(),
            model: default_model(),
            workdir: default_workdir(),
        }
    }
}

fn load_config() -> Config {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()));

    let candidates = [
        exe_dir.as_ref().map(|d| d.join("config.json")),
        Some(PathBuf::from("config.json")),
    ];

    for path in candidates.iter().flatten() {
        if let Ok(text) = std::fs::read_to_string(path) {
            match serde_json::from_str::<Config>(&text) {
                Ok(cfg) => {
                    eprintln!("[config] loaded from {}", path.display());
                    return cfg;
                }
                Err(e) => {
                    eprintln!("[config] parse error in {}: {}", path.display(), e);
                }
            }
        }
    }

    eprintln!("[config] no config.json found, using defaults");
    Config::default()
}

fn resolve_workdir(configured: &str) -> PathBuf {
    if configured != "." && !configured.is_empty() {
        let p = PathBuf::from(configured);
        if p.is_dir() {
            return p;
        }
        eprintln!("[config] workdir '{}' does not exist, asking user...", configured);
    }

    eprintln!("[config] no workdir configured, opening folder picker...");
    if let Some(path) = rfd::FileDialog::new()
        .set_title("Select workspace folder")
        .pick_folder()
    {
        return path;
    }

    eprintln!("[config] no folder selected, exiting.");
    std::process::exit(0);
}

// --- プロンプト読み込み ---

/// prompts/ ディレクトリからテキストファイルを読む。
/// strip_comments=true なら # で始まるコメント行を除去する。
/// ファイルが見つからなければ fallback を返す。
fn load_prompt(name: &str, fallback: &str, strip_comments: bool) -> String {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()));

    let candidates = [
        exe_dir.as_ref().map(|d| d.join("prompts").join(name)),
        Some(PathBuf::from("prompts").join(name)),
    ];

    for path in candidates.iter().flatten() {
        if let Ok(text) = std::fs::read_to_string(path) {
            eprintln!("[prompt] loaded {} from {}", name, path.display());
            if strip_comments {
                let filtered: String = text.lines()
                    .filter(|line| !line.starts_with('#'))
                    .collect::<Vec<_>>()
                    .join("\n");
                return filtered.trim().to_string();
            }
            return text;
        }
    }

    eprintln!("[prompt] {} not found, using built-in default", name);
    fallback.to_string()
}

// --- テスト API ---

/// メインスレッド (EventLoop) に送るイベント
#[derive(Debug)]
enum TestEvent {
    /// チャットにメッセージを送信
    SendMessage(String),
    /// JS に状態レポートを要求
    RequestState,
}

/// JS から IPC 経由で受け取った最新の状態
type SharedState = Arc<Mutex<String>>;

/// 簡易 HTTP サーバー (テスト自動化用)
fn start_test_api(proxy: EventLoopProxy<TestEvent>, shared_state: SharedState) {
    thread::spawn(move || {
        let listener = match TcpListener::bind(format!("127.0.0.1:{}", TEST_API_PORT)) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("[test-api] failed to bind port {}: {}", TEST_API_PORT, e);
                return;
            }
        };
        eprintln!("[test-api] listening on http://127.0.0.1:{}", TEST_API_PORT);

        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut reader = BufReader::new(stream.try_clone().unwrap());

            // リクエスト行をパース
            let mut request_line = String::new();
            if reader.read_line(&mut request_line).is_err() { continue; }
            let parts: Vec<&str> = request_line.trim().split(' ').collect();
            if parts.len() < 2 { continue; }
            let method = parts[0];
            let path = parts[1];

            // ヘッダーを読み飛ばし、Content-Length を取得
            let mut content_length: usize = 0;
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).is_err() { break; }
                let trimmed = line.trim();
                if trimmed.is_empty() { break; }
                if let Some(val) = trimmed.strip_prefix("Content-Length:") {
                    content_length = val.trim().parse().unwrap_or(0);
                }
                // case-insensitive
                if let Some(val) = trimmed.strip_prefix("content-length:") {
                    content_length = val.trim().parse().unwrap_or(0);
                }
            }

            // ボディを読む
            let mut body = vec![0u8; content_length];
            if content_length > 0 {
                let _ = std::io::Read::read_exact(&mut reader, &mut body);
            }
            let body_str = String::from_utf8_lossy(&body).to_string();

            // CORS ヘッダー
            let cors = "Access-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type\r\n";

            match (method, path) {
                ("OPTIONS", _) => {
                    let _ = write!(stream, "HTTP/1.1 204 No Content\r\n{cors}\r\n");
                }
                ("POST", "/api/send") => {
                    // {"text": "..."} をパースしてメッセージ送信
                    let text = serde_json::from_str::<serde_json::Value>(&body_str)
                        .ok()
                        .and_then(|v| v.get("text").and_then(|t| t.as_str()).map(String::from))
                        .unwrap_or(body_str.clone());

                    let _ = proxy.send_event(TestEvent::SendMessage(text.clone()));
                    let resp = format!("{{\"ok\":true,\"sent\":{}}}", serde_json::json!(text));
                    let _ = write!(stream,
                        "HTTP/1.1 200 OK\r\n{cors}Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                        resp.len(), resp
                    );
                    eprintln!("[test-api] POST /api/send: {}", text.chars().take(80).collect::<String>());
                }
                ("GET", "/api/messages") => {
                    // まず JS に状態レポートを要求
                    let _ = proxy.send_event(TestEvent::RequestState);
                    // IPC コールバックが状態を更新するのを少し待つ
                    thread::sleep(std::time::Duration::from_millis(200));
                    let state = shared_state.lock().unwrap().clone();
                    let _ = write!(stream,
                        "HTTP/1.1 200 OK\r\n{cors}Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                        state.len(), state
                    );
                    eprintln!("[test-api] GET /api/messages ({} bytes)", state.len());
                }
                ("GET", "/api/health") => {
                    let resp = r#"{"ok":true}"#;
                    let _ = write!(stream,
                        "HTTP/1.1 200 OK\r\n{cors}Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                        resp.len(), resp
                    );
                }
                _ => {
                    let resp = r#"{"error":"not found"}"#;
                    let _ = write!(stream,
                        "HTTP/1.1 404 Not Found\r\n{cors}Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                        resp.len(), resp
                    );
                }
            }
        }
    });
}

fn main() {
    let config = load_config();
    let workdir = resolve_workdir(&config.workdir);

    eprintln!("[config] endpoint={} model={} workdir={}",
        config.endpoint, config.model, workdir.display());

    let system_prompt = load_prompt("system.txt", "You are a helpful coding assistant.", true);
    let tools_json = load_prompt("tools.json", "[]", false);

    // tools.json をパースして妥当性を確認（壊れていたら空配列にフォールバック）
    let tools_json_clean = match serde_json::from_str::<serde_json::Value>(&tools_json) {
        Ok(v) => {
            let count = v.as_array().map(|a| a.len()).unwrap_or(0);
            eprintln!("[prompt] tools.json parsed ({} tools)", count);
            v.to_string()
        }
        Err(e) => {
            eprintln!("[prompt] tools.json parse error: {} — using empty tool list", e);
            "[]".to_string()
        }
    };

    let html = HTML
        .replace("{{MARKED_JS}}", MARKED_JS)
        .replace("{{ENDPOINT}}", &config.endpoint)
        .replace("{{MODEL}}", &config.model)
        .replace("{{WORKDIR}}", &workdir.display().to_string())
        .replace("{{SYSTEM_PROMPT}}", &system_prompt.replace('\\', "\\\\").replace('`', "\\`").replace("${", "\\${"))
        .replace("{{TOOLS_JSON}}", &tools_json_clean);

    let state = RefCell::new(tools::EditorState::new(workdir));
    let shared_state: SharedState = Arc::new(Mutex::new("[]".into()));

    let event_loop = EventLoopBuilder::<TestEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    // テスト API サーバー起動
    start_test_api(proxy, shared_state.clone());

    let menu = Menu::new();
    let edit_menu = Submenu::new("Edit", true);
    edit_menu.append(&PredefinedMenuItem::undo(None)).unwrap();
    edit_menu.append(&PredefinedMenuItem::redo(None)).unwrap();
    edit_menu.append(&PredefinedMenuItem::separator()).unwrap();
    edit_menu.append(&PredefinedMenuItem::cut(None)).unwrap();
    edit_menu.append(&PredefinedMenuItem::copy(None)).unwrap();
    edit_menu.append(&PredefinedMenuItem::paste(None)).unwrap();
    edit_menu.append(&PredefinedMenuItem::select_all(None)).unwrap();
    menu.append(&edit_menu).unwrap();

    #[cfg(target_os = "macos")]
    menu.init_for_nsapp();

    let window = WindowBuilder::new()
        .with_title("Chat Client — Tools API Demo")
        .with_inner_size(tao::dpi::LogicalSize::new(900.0, 700.0))
        .with_min_inner_size(tao::dpi::LogicalSize::new(500.0, 400.0))
        .build(&event_loop)
        .expect("Failed to create window");

    // IPC 用に shared_state のクローン
    let ipc_state = shared_state.clone();

    // with_html だと origin が null になり、カスタムプロトコルへの fetch がブロックされる。
    // HTML とツール IPC を同一の "app://" プロトコルでサーブし、パスでルーティングする。
    //   app://localhost/         → HTML をサーブ
    //   app://localhost/tool     → ツール実行 (POST)
    let html_for_app = html.clone();
    let webview = WebViewBuilder::new()
        .with_custom_protocol("app".into(), move |_webview_id, request| {
            let uri = request.uri().to_string();

            if uri.contains("/tool") {
                // ツール実行リクエスト
                let body = request.body();
                let body_str = String::from_utf8_lossy(body);

                let result = match serde_json::from_str::<serde_json::Value>(&body_str) {
                    Ok(req) => {
                        let tool_name = req.get("tool").and_then(|v| v.as_str()).unwrap_or("");
                        let args = req.get("args").unwrap_or(&serde_json::Value::Null);
                        tools::dispatch(&state, tool_name, args)
                    }
                    Err(e) => format!("ERROR: Invalid request: {}", e),
                };

                let body_bytes: Cow<'static, [u8]> = Cow::Owned(result.into_bytes());
                Response::builder()
                    .header(CONTENT_TYPE, "text/plain; charset=utf-8")
                    .body(body_bytes)
                    .unwrap()
            } else {
                // HTML をサーブ
                let body_bytes: Cow<'static, [u8]> = Cow::Owned(html_for_app.clone().into_bytes());
                Response::builder()
                    .header(CONTENT_TYPE, "text/html; charset=utf-8")
                    .body(body_bytes)
                    .unwrap()
            }
        })
        .with_url("app://localhost")
        .with_focused(true)
        .with_ipc_handler(move |request| {
            // JS から window.ipc.postMessage() で送られてくる状態を保存
            let body = request.body();
            *ipc_state.lock().unwrap() = body.clone();
        })
        .build(&window)
        .expect("Failed to create webview");

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::UserEvent(test_event) => {
                match test_event {
                    TestEvent::SendMessage(text) => {
                        // JS のエスケープ
                        let escaped = text
                            .replace('\\', "\\\\")
                            .replace('\'', "\\'")
                            .replace('\n', "\\n")
                            .replace('\r', "");
                        let js = format!("__testSend('{}')", escaped);
                        webview.evaluate_script(&js).ok();
                    }
                    TestEvent::RequestState => {
                        webview.evaluate_script("__testReportState()").ok();
                    }
                }
            }
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => {
                *control_flow = ControlFlow::Exit;
            }
            _ => {}
        }
    });
}

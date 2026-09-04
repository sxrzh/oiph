//! 终端辅助：raw 模式输出适配、双 Esc 打断。

use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::{Notify, mpsc::UnboundedSender};

thread_local! {
    static RAW_MODE: Cell<bool> = const { Cell::new(false) };
}

// WS sender 用全局 Mutex 而非 thread_local：
// tokio 任务在 await 点可能迁移 worker 线程，thread_local 会导致
// set 与 send 落在不同线程，WS 消息静默丢失。
static WS_SENDER: Mutex<Option<UnboundedSender<String>>> = Mutex::new(None);

/// 设置 WebSocket 发送器（Web GUI 模式下用），将日志/内容转发给前端。
pub fn set_ws_sender(sender: Option<UnboundedSender<String>>) {
    *WS_SENDER.lock().unwrap() = sender;
}

fn ws_send(msg: &str) {
    if let Ok(guard) = WS_SENDER.lock()
        && let Some(sender) = guard.as_ref() {
            let _ = sender.send(msg.to_string());
        }
}

/// 当前是否处于 GUI（WS 转发）模式。
pub fn ws_active() -> bool {
    WS_SENDER.lock().map(|g| g.is_some()).unwrap_or(false)
}

pub fn set_raw(on: bool) {
    RAW_MODE.with(|c| c.set(on));
}

pub fn is_raw() -> bool {
    RAW_MODE.with(|c| c.get())
}

/// stdout 输出，raw 模式下把 `\n` 转 `\r\n`。
pub fn print_out(text: &str) {
    if is_raw() {
        print!("{}", text.replace('\n', "\r\n"));
    } else {
        print!("{}", text);
    }
    let _ = std::io::Write::flush(&mut std::io::stdout());
}

/// stderr 输出，raw 模式下把 `\n` 转 `\r\n`。同时转发到 WebSocket（若设置）。
pub fn print_err(text: &str) {
    if is_raw() {
        eprint!("{}", text.replace('\n', "\r\n"));
    } else {
        eprint!("{}", text);
    }
    let _ = std::io::Write::flush(&mut std::io::stderr());
    ws_send(&format!(
        r#"{{"type":"log","text":{}}}"#,
        serde_json::to_string(text).unwrap_or_default()
    ));
}

pub fn println_err(text: &str) {
    print_err(text);
    print_err("\n");
}

/// 空操作，供子 Agent 传递给 chat_stream 的 on_content 回调。
pub fn noop(_: &str) {}

/// 打印流式内容（供 supervisor 的 on_content 回调）。同时转发到 WebSocket。
pub fn print_content(text: &str) {
    print_out(text);
    ws_send(&format!(
        r#"{{"type":"content","text":{}}}"#,
        serde_json::to_string(text).unwrap_or_default()
    ));
}

/// Token 用量 JSON 值转发（GUI 状态栏实时更新）。
/// 本回合精确用量的增量更新（GUI 在全局基线上累加）。
pub fn send_usage_turn(usage_json: &str) {
    ws_send(&format!(r#"{{"type":"usage_turn","usage":{usage_json}}}"#));
}

/// 流式估算的增量（本次流自己的输入/输出估算，显示层累加到累计值上）。
pub fn send_usage_live(input: u64, output: u64) {
    ws_send(&format!(
        r#"{{"type":"usage_live","input":{input},"output":{output}}}"#
    ));
}

/// 工具调用结构化消息（GUI 完整显示，不截断）。
pub fn send_tool_call(name: &str, args: &serde_json::Value) {
    ws_send(&format!(
        r#"{{"type":"tool_call","name":{},"args":{}}}"#,
        serde_json::to_string(name).unwrap_or_default(),
        serde_json::to_string(args).unwrap_or_default(),
    ));
}

/// 工具结果结构化消息（GUI 完整显示，不截断）。
pub fn send_tool_result(result: &str) {
    ws_send(&format!(
        r#"{{"type":"tool_result","text":{}}}"#,
        serde_json::to_string(result).unwrap_or_default(),
    ));
}

/// 步骤边界：通知前端新开一条消息（思维链/内容），并告知当前 agent。
pub fn send_step_boundary(agent: &str) {
    ws_send(&format!(
        r#"{{"type":"step_boundary","agent":{}}}"#,
        serde_json::to_string(agent).unwrap_or_default()
    ));
}

/// 向前端推送问卷（ask_user 工具）。questions 为 JSON 数组字符串。
pub fn send_ask_user(questions_json: &str) {
    ws_send(&format!(
        r#"{{"type":"ask_user","questions":{questions_json}}}"#
    ));
}

/// 打印思维链。CLI 模式过长时只显示最后 300 字符（GUI 模式显示完整）。
/// 输出到 stderr（不干扰 stdout 内容流），加灰色前缀。
static REASONING_BUF: Mutex<String> = Mutex::new(String::new());

pub fn reset_reasoning_buf() {
    if let Ok(mut buf) = REASONING_BUF.lock() {
        buf.clear();
    }
    // CLI 单行覆盖模式需要清行；GUI 模式直接拼接不需要
    if !ws_active() {
        print_err("\r\x1b[K");
    }
}

pub fn print_reasoning(text: &str) {
    // GUI 模式：思维链流式直接拼接输出（不逐块重打整行、不换行）
    if ws_active() {
        let first = {
            let mut buf = REASONING_BUF.lock().unwrap();
            let empty = buf.is_empty();
            buf.push_str(text);
            empty
        };
        if first {
            print_err("\n\x1b[2m[思维链]\x1b[0m ");
        }
        print_err(text);
        // WebSocket 转发原始 delta
        ws_send(&format!(
            r#"{{"type":"reasoning","text":{}}}"#,
            serde_json::to_string(text).unwrap_or_default()
        ));
        return;
    }

    // CLI 模式：单行覆盖显示，过长截断
    let buf_display;
    {
        let mut buf = REASONING_BUF.lock().unwrap();
        buf.push_str(text);
        buf_display = buf.clone();
    }
    let len = buf_display.chars().count();
    let display: String = if len > 300 {
        format!("...{}", buf_display.chars().skip(len - 300).collect::<String>())
    } else {
        buf_display
    };
    // 回车到行首覆盖
    print_err("\r");
    print_err(&format!("\x1b[2m[思维链] {}\x1b[0m\r", display));
    // WebSocket 转发原始 delta（CLI 模式无 WS，noop）
    ws_send(&format!(
        r#"{{"type":"reasoning","text":{}}}"#,
        serde_json::to_string(text).unwrap_or_default()
    ));
}

// ---------------------------------------------------------------------------
// CancelFlag
// ---------------------------------------------------------------------------

/// 打断标志：AtomicBool 用于检查，Notify 用于唤醒正在 await 的流读取。
#[derive(Clone)]
pub struct CancelFlag {
    cancelled: Arc<AtomicBool>,
    notify: Arc<Notify>,
}

impl CancelFlag {
    pub fn new() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
            notify: Arc::new(Notify::new()),
        }
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
        self.notify.notify_waiters();
    }

    /// 重置取消标志（新回合开始时调用）。
    pub fn reset(&self) {
        self.cancelled.store(false, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }

    pub async fn wait(&self) {
        self.notify.notified().await;
    }
}

impl Default for CancelFlag {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// EscWatcher
// ---------------------------------------------------------------------------

/// 双 Esc 打断监视器。启动一个后台线程在 raw 模式下监听按键，
/// 两次 Esc（500ms 内）触发 cancel。
pub struct EscWatcher {
    stop: Arc<AtomicBool>,
}

impl EscWatcher {
    /// 启动监视器。调用方需在此之前已 `enable_raw_mode`。
    pub fn start(cancel: CancelFlag) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let stop2 = stop.clone();
        std::thread::spawn(move || {
            let mut last_esc: Option<Instant> = None;
            while !stop2.load(Ordering::Relaxed) {
                if crossterm::event::poll(Duration::from_millis(100)).unwrap_or(false)
                    && let Ok(crossterm::event::Event::Key(k)) = crossterm::event::read()
                        && let crossterm::event::KeyCode::Esc = k.code {
                            let now = Instant::now();
                            if let Some(last) = last_esc
                                && now.duration_since(last) < Duration::from_millis(500)
                            {
                                cancel.cancel();
                                break;
                            }
                            last_esc = Some(now);
                        }
            }
        });
        Self { stop }
    }
}

impl Drop for EscWatcher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancel_flag_works() {
        let f = CancelFlag::new();
        assert!(!f.is_cancelled());
        f.cancel();
        assert!(f.is_cancelled());
    }
}

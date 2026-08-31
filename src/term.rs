//! 终端辅助：raw 模式输出适配、双 Esc 打断。

use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Notify;

thread_local! {
    static RAW_MODE: Cell<bool> = const { Cell::new(false) };
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

/// stderr 输出，raw 模式下把 `\n` 转 `\r\n`。
pub fn print_err(text: &str) {
    if is_raw() {
        eprint!("{}", text.replace('\n', "\r\n"));
    } else {
        eprint!("{}", text);
    }
    let _ = std::io::Write::flush(&mut std::io::stderr());
}

pub fn println_err(text: &str) {
    print_err(text);
    print_err("\n");
}

/// 空操作，供子 Agent 传递给 chat_stream 的 on_content 回调。
pub fn noop(_: &str) {}

/// 打印流式内容（供 supervisor 的 on_content 回调）。
pub fn print_content(text: &str) {
    print_out(text);
}

/// 打印思维链。过长时只显示最后 300 字符，前面用 ... 替代。
/// 输出到 stderr（不干扰 stdout 内容流），加灰色前缀。
use std::sync::Mutex;
static REASONING_BUF: Mutex<String> = Mutex::new(String::new());

pub fn reset_reasoning_buf() {
    if let Ok(mut buf) = REASONING_BUF.lock() {
        buf.clear();
    }
    // 清除行
    print_err("\r\x1b[K");
}

pub fn print_reasoning(text: &str) {
    let mut buf = REASONING_BUF.lock().unwrap();
    buf.push_str(text);
    let len = buf.chars().count();
    let display: String = if len > 300 {
        format!("...{}", buf.chars().skip(len - 300).collect::<String>())
    } else {
        buf.clone()
    };
    // 回车到行首覆盖
    print_err("\r");
    print_err(&format!("\x1b[2m[思维链] {}\x1b[0m\r", display.chars().take(200).collect::<String>()));
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

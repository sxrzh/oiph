//! OpenAI 兼容的 LLM 客户端。流式输出、指数退避重试、Token 用量统计。

use std::time::Duration;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tokio_stream::StreamExt;

use crate::term::CancelFlag;

/// 简单的随机抖动：返回 [0, max_ms) 范围内的毫秒数。
fn rand_jitter_ms(max_ms: u64) -> u64 {
    use std::cell::Cell;
    thread_local! {
        static SEED: Cell<u64> = Cell::new(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(42)
                | 1,
        );
    }
    SEED.with(|s| {
        let mut x = s.get();
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        s.set(x);
        x % max_ms
    })
}

// ---------------------------------------------------------------------------
// 数据结构
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(default, rename = "reasoning_content", skip_serializing)]
    pub reasoning: Option<String>,
}

impl Message {
    pub fn system(text: impl Into<String>) -> Self {
        Self { role: "system".into(), content: Some(text.into()), tool_calls: None, tool_call_id: None, reasoning: None }
    }
    pub fn user(text: impl Into<String>) -> Self {
        Self { role: "user".into(), content: Some(text.into()), tool_calls: None, tool_call_id: None, reasoning: None }
    }
    pub fn tool(result: String, tool_call_id: String) -> Self {
        Self { role: "tool".into(), content: Some(result), tool_calls: None, tool_call_id: Some(tool_call_id), reasoning: None }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub function: FunctionCall,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct Tool {
    #[serde(rename = "type")]
    pub kind: String,
    pub function: FunctionDef,
}

#[derive(Debug, Serialize, Clone)]
pub struct FunctionDef {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: &'a [Message],
    tools: &'a [Tool],
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream_options: Option<StreamOptions>,
}

#[derive(Debug, Serialize)]
struct StreamOptions {
    include_usage: bool,
}

/// Token 用量。
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct ChatUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    pub cache_hit_tokens: Option<u64>,
    pub cache_miss_tokens: Option<u64>,
}

/// 单次 chat 调用的结果。
#[derive(Debug)]
pub struct ChatResult {
    pub message: Message,
    pub usage: Option<ChatUsage>,
    pub interrupted: bool,
}

/// 粗略 token 估算：CJK 字符约 0.6 token/字，其他约 0.25 token/字符。
fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x4E00..=0x9FFF | 0x3400..=0x4DBF | 0x3000..=0x303F | 0xFF00..=0xFFEF)
}

fn estimate_text_tokens(text: &str) -> f64 {
    let mut cjk = 0usize;
    let mut other = 0usize;
    for c in text.chars() {
        if is_cjk(c) { cjk += 1 } else { other += 1 }
    }
    cjk as f64 * 0.6 + other as f64 * 0.25
}

fn estimate_message_tokens(m: &Message) -> f64 {
    let mut n = 8.0; // 每条消息固定开销
    if let Some(c) = &m.content {
        n += estimate_text_tokens(c);
    }
    if let Some(tcs) = &m.tool_calls {
        for tc in tcs {
            n += estimate_text_tokens(&tc.function.name)
                + estimate_text_tokens(&tc.function.arguments)
                + 8.0;
        }
    }
    n
}

// ---------------------------------------------------------------------------
// 流式 SSE 解析结构
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct StreamChunk {
    #[serde(default)]
    choices: Vec<StreamChoice>,
    #[serde(default)]
    usage: Option<RawUsage>,
}

#[derive(Deserialize)]
struct StreamChoice {
    #[serde(default)]
    delta: Delta,
}

#[derive(Deserialize, Default)]
struct Delta {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<ToolCallDelta>>,
    #[serde(default, rename = "reasoning_content")]
    reasoning: Option<String>,
}

#[derive(Deserialize)]
struct ToolCallDelta {
    index: usize,
    #[serde(default)]
    id: Option<String>,
    #[serde(rename = "type", default)]
    kind: Option<String>,
    #[serde(default)]
    function: Option<FunctionDelta>,
}

#[derive(Deserialize, Default)]
struct FunctionDelta {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

#[derive(Deserialize)]
struct RawUsage {
    prompt_tokens: u64,
    completion_tokens: u64,
    #[serde(default)]
    total_tokens: Option<u64>,
    #[serde(default)]
    prompt_cache_hit_tokens: Option<u64>,
    #[serde(default)]
    prompt_cache_miss_tokens: Option<u64>,
}

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    base_url: String,
    api_key: String,
}

impl Client {
    pub fn new(base_url: String, api_key: String) -> Result<Self> {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(30))
            // 总超时放宽到 30 分钟：大响应（长思维链/大量工具调用参数）
            // 流式生成可能远超 180s，超时会把流掐断报 decode 错误
            .timeout(Duration::from_secs(1800))
            // 空闲超时：两个 chunk 之间超过 120s 无数据才判定断流
            .read_timeout(Duration::from_secs(120))
            .build()
            .context("构造 HTTP 客户端失败")?;
        Ok(Self { http, base_url, api_key })
    }

    /// 流式调用 chat/completions。支持指数退避重试、Token 用量、双 Esc 打断。
    ///
    /// `on_content` 回调在每个 content delta 到达时被调用（用于实时打印）。
    pub async fn chat_stream(
        &self,
        model: &str,
        messages: &[Message],
        tools: &[Tool],
        cancel: &CancelFlag,
        on_content: fn(&str),
        on_reasoning: fn(&str),
    ) -> Result<ChatResult> {
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let body = ChatRequest {
            model,
            messages,
            tools,
            stream: true,
            stream_options: Some(StreamOptions { include_usage: true }),
        };

        // Phase 1: 发送请求（含重试）
        let resp = self.send_with_retry(&url, &body).await?;

        // Phase 2: 流式读取 SSE（含打断）
        self.stream_response(resp, messages, cancel, on_content, on_reasoning).await
    }

    /// 发送请求，含指数退避重试（网络错误 / 429 / 5xx）。
    async fn send_with_retry(&self, url: &str, body: &ChatRequest<'_>) -> Result<reqwest::Response> {
        const MAX_RETRIES: usize = 5;
        const BACKOFF_SECS: [u64; MAX_RETRIES] = [1, 2, 4, 8, 8];
        const JITTER_MAX_MS: u64 = 500;

        let mut last_err: Option<anyhow::Error> = None;
        for attempt in 0..=MAX_RETRIES {
            if attempt > 0 {
                let base = BACKOFF_SECS[attempt - 1];
                let jitter = rand_jitter_ms(JITTER_MAX_MS);
                crate::term::println_err(&format!(
                    "[client] 第 {attempt} 次重试，等待 {}ms", base * 1000 + jitter
                ));
                tokio::time::sleep(Duration::from_millis(base * 1000 + jitter)).await;
            }

            match self.http.post(url).bearer_auth(&self.api_key).json(body).send().await {
                Ok(resp) => {
                    let status = resp.status();
                    if status.is_success() {
                        return Ok(resp);
                    }
                    let text = resp.text().await.unwrap_or_default();
                    let code = status.as_u16();
                    let is_retryable = code == 429 || (500..600).contains(&code);
                    last_err = Some(anyhow::anyhow!(
                        "模型供应商错误（HTTP {status}）：\n{}",
                        &text[..text.len().min(500)]
                    ));
                    if !is_retryable || attempt == MAX_RETRIES {
                        return Err(last_err.unwrap());
                    }
                }
                Err(e) => {
                    last_err = Some(anyhow::Error::new(e).context("请求模型供应商失败"));
                    if attempt == MAX_RETRIES {
                        return Err(last_err.unwrap());
                    }
                }
            }
        }
        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("未知错误")))
    }

    /// 从成功响应中流式读取 SSE，解析 delta、累积 content/tool_calls、收集 usage。
    async fn stream_response(
        &self,
        resp: reqwest::Response,
        messages: &[Message],
        cancel: &CancelFlag,
        on_content: fn(&str),
        on_reasoning: fn(&str),
    ) -> Result<ChatResult> {
        let mut stream = resp.bytes_stream();
        let mut buf = String::new();
        let mut content = String::new();
        let mut reasoning = String::new();
        let mut tool_calls: Vec<ToolCall> = Vec::new();
        let mut usage: Option<ChatUsage> = None;

        // 流式期间的实时用量估算（API 只在流末尾给精确值）。
        // 输入/输出（含思维链）都估算：CJK 字符约 0.6 token，其他约 0.25 token/字符。
        let est_prompt = messages.iter().map(estimate_message_tokens).sum::<f64>();
        let mut streamed_cjk = 0usize;
        let mut streamed_other = 0usize;
        let mut last_usage_push = std::time::Instant::now();
        let send_live_usage = |est_prompt: f64, cjk: usize, other: usize| {
            let completion = (cjk as f64 * 0.6 + other as f64 * 0.25).round() as u64;
            let prompt = est_prompt.round() as u64;
            crate::term::send_usage_json(&serde_json::json!({
                "prompt_tokens": prompt,
                "completion_tokens": completion,
                "total_tokens": prompt + completion,
            }).to_string());
        };

        loop {
            tokio::select! {
                chunk = stream.next() => {
                    match chunk {
                        Some(Ok(bytes)) => {
                            buf.push_str(&String::from_utf8_lossy(&bytes));
                            while let Some(pos) = buf.find("\n\n") {
                                let event = buf[..pos].to_string();
                                buf = buf[pos + 2..].to_string();
                                let event = event.replace("\r\n", "\n");
                                for line in event.lines() {
                                    if let Some(data) = line.strip_prefix("data: ") {
                                        if data == "[DONE]" {
                                            // 流结束
                                            return Ok(self.build_result(content, reasoning, tool_calls, usage, false));
                                        }
                                        match serde_json::from_str::<StreamChunk>(data) {
                                            Ok(chunk) => {
                                                for choice in &chunk.choices {
                                                    if let Some(c) = &choice.delta.content {
                                                        content.push_str(c);
                                                        on_content(c);
                                                        for ch in c.chars() {
                                                            if is_cjk(ch) { streamed_cjk += 1 } else { streamed_other += 1 }
                                                        }
                                                    }
                                                    if let Some(r) = &choice.delta.reasoning {
                                                        reasoning.push_str(r);
                                                        on_reasoning(r);
                                                        for ch in r.chars() {
                                                            if is_cjk(ch) { streamed_cjk += 1 } else { streamed_other += 1 }
                                                        }
                                                    }
                                                    if let Some(tcs) = &choice.delta.tool_calls {
                                                        for tc in tcs {
                                                            // 按 index 扩展或追加
                                                            while tool_calls.len() <= tc.index {
                                                                tool_calls.push(ToolCall {
                                                                    id: String::new(),
                                                                    kind: "function".into(),
                                                                    function: FunctionCall { name: String::new(), arguments: String::new() },
                                                                });
                                                            }
                                                            let slot = &mut tool_calls[tc.index];
                                                            if let Some(id) = &tc.id { slot.id = id.clone(); }
                                                            if let Some(k) = &tc.kind { slot.kind = k.clone(); }
                                                            if let Some(f) = &tc.function {
                                                                if let Some(n) = &f.name { slot.function.name = n.clone(); }
                                                                if let Some(a) = &f.arguments { slot.function.arguments.push_str(a); }
                                                            }
                                                        }
                                                    }
                                                }
                                                if let Some(u) = &chunk.usage {
                                                    // 精确用量到达（含思维链部分），覆盖估算值
                                                    usage = Some(ChatUsage {
                                                        prompt_tokens: u.prompt_tokens,
                                                        completion_tokens: u.completion_tokens,
                                                        total_tokens: u.total_tokens.unwrap_or(u.prompt_tokens + u.completion_tokens),
                                                        cache_hit_tokens: u.prompt_cache_hit_tokens,
                                                        cache_miss_tokens: u.prompt_cache_miss_tokens,
                                                    });
                                                    crate::term::send_usage_json(&serde_json::to_string(usage.as_ref().unwrap()).unwrap_or_default());
                                                }
                                                // 流式期间每 800ms 推送一次实时估算（思维链接收中也要更新）
                                                if usage.is_none()
                                                    && last_usage_push.elapsed() >= std::time::Duration::from_millis(800)
                                                {
                                                    last_usage_push = std::time::Instant::now();
                                                    send_live_usage(est_prompt, streamed_cjk, streamed_other);
                                                }
                                            }
                                            Err(e) => {
                                                crate::term::println_err(&format!("[client] SSE 解析失败：{e}"));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        Some(Err(e)) => {
                            return Err(anyhow::anyhow!("读取流失败：{e}"));
                        }
                        None => {
                            // 连接正常关闭
                            return Ok(self.build_result(content, reasoning, tool_calls, usage, false));
                        }
                    }
                }
                _ = cancel.wait() => {
                    return Ok(self.build_result(content, reasoning, tool_calls, usage, true));
                }
            }
        }
    }

    fn build_result(
        &self,
        content: String,
        reasoning: String,
        tool_calls: Vec<ToolCall>,
        usage: Option<ChatUsage>,
        interrupted: bool,
    ) -> ChatResult {
        let has_tool_calls = !tool_calls.is_empty();
        ChatResult {
            message: Message {
                role: "assistant".into(),
                content: if content.is_empty() && has_tool_calls { None } else { Some(content) },
                tool_calls: if has_tool_calls { Some(tool_calls) } else { None },
                tool_call_id: None,
                reasoning: if reasoning.is_empty() { None } else { Some(reasoning) },
            },
            usage,
            interrupted,
        }
    }

    /// 调用 OpenAI 兼容 `/embeddings` 端点，分批返回向量。
    pub async fn embeddings(&self, model: &str, inputs: &[String]) -> Result<Vec<Vec<f32>>> {
        const BATCH: usize = 16;
        let url = format!("{}/embeddings", self.base_url.trim_end_matches('/'));
        let mut out = Vec::with_capacity(inputs.len());
        for batch in inputs.chunks(BATCH) {
            #[derive(Serialize)]
            struct EmbedReq<'a> { model: &'a str, input: &'a [String] }
            #[derive(Deserialize)]
            struct EmbedData { embedding: Vec<f32>, index: usize }
            #[derive(Deserialize)]
            struct EmbedResp { data: Vec<EmbedData> }

            let resp = self.http.post(&url).bearer_auth(&self.api_key)
                .json(&EmbedReq { model, input: batch }).send().await
                .context("请求 embeddings 失败")?;
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            if !status.is_success() {
                anyhow::bail!("embeddings 错误（HTTP {}）：\n{}", status, text);
            }
            let parsed: EmbedResp = serde_json::from_str(&text)
                .with_context(|| format!("解析 embeddings 响应失败：\n{text}"))?;
            let mut data = parsed.data;
            data.sort_by_key(|d| d.index);
            anyhow::ensure!(data.len() == batch.len(), "embeddings 返回 {} 个向量，期望 {} 个", data.len(), batch.len());
            out.extend(data.into_iter().map(|d| d.embedding));
        }
        Ok(out)
    }
}

// ---------------------------------------------------------------------------
// 用量展示与定价估算
// ---------------------------------------------------------------------------

/// 按模型名估算价格（USD）。返回 (input_per_M, output_per_M, cache_hit_per_M)。
/// 不在表中的模型返回 None。
pub fn model_pricing(model: &str) -> Option<(f64, f64, f64)> {
    let m = model.to_lowercase();
    if m.contains("deepseek-v4-flash") {
        Some((0.14, 0.28, 0.014))
    } else if m.contains("deepseek-chat") || m.contains("deepseek-v3") {
        Some((0.27, 1.10, 0.07))
    } else if m.contains("deepseek-r1") {
        Some((0.55, 2.19, 0.14))
    } else if m.contains("gpt-4o") {
        Some((2.50, 10.0, 1.25))
    } else if m.contains("gpt-4o-mini") {
        Some((0.15, 0.60, 0.075))
    } else {
        None
    }
}

/// 估算费用（USD）。
pub fn estimate_price(model: &str, usage: &ChatUsage) -> Option<f64> {
    let (in_p, out_p, cache_p) = model_pricing(model)?;
    let cache_hit = usage.cache_hit_tokens.unwrap_or(0);
    let prompt_no_cache = usage.prompt_tokens.saturating_sub(cache_hit);
    let price = (prompt_no_cache as f64 * in_p + cache_hit as f64 * cache_p + usage.completion_tokens as f64 * out_p) / 1_000_000.0;
    Some(price)
}

/// 格式化用量摘要。
pub fn format_usage(model: &str, usage: &ChatUsage) -> String {
    let mut parts = vec![
        format!("输入 {}", usage.prompt_tokens),
        format!("输出 {}", usage.completion_tokens),
    ];
    if let Some(hit) = usage.cache_hit_tokens {
        let total = usage.prompt_tokens.max(1) as f64;
        let rate = hit as f64 / total * 100.0;
        parts.push(format!("缓存命中 {}（{:.1}%）", hit, rate));
    }
    let mut out = format!("📊 Token 用量：{}", parts.join(" / "));
    if let Some(price) = estimate_price(model, usage) {
        out.push_str(&format!(" / 估计费用 ${:.4}", price));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reasoning_is_never_serialized_back() {
        let msg = Message {
            role: "assistant".into(),
            content: Some("final".into()),
            tool_calls: None,
            tool_call_id: None,
            reasoning: Some("secret chain of thought".into()),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("final"));
        assert!(!json.contains("chain of thought"));
        assert!(!json.contains("reasoning"));
    }

    #[test]
    fn stream_chunk_parse() {
        let json = r#"{"choices":[{"delta":{"content":"hello"},"finish_reason":null}],"usage":null}"#;
        let chunk: StreamChunk = serde_json::from_str(json).unwrap();
        assert_eq!(chunk.choices[0].delta.content.as_deref(), Some("hello"));
    }

    #[test]
    fn stream_chunk_usage() {
        let json = r#"{"choices":[],"usage":{"prompt_tokens":100,"completion_tokens":50,"total_tokens":150,"prompt_cache_hit_tokens":80}}"#;
        let chunk: StreamChunk = serde_json::from_str(json).unwrap();
        let u = chunk.usage.unwrap();
        assert_eq!(u.prompt_tokens, 100);
        assert_eq!(u.completion_tokens, 50);
        assert_eq!(u.prompt_cache_hit_tokens, Some(80));
    }

    #[test]
    fn estimate_price_works() {
        let usage = ChatUsage {
            prompt_tokens: 1000,
            completion_tokens: 500,
            total_tokens: 1500,
            cache_hit_tokens: Some(800),
            cache_miss_tokens: None,
        };
        let price = estimate_price("deepseek-v4-flash", &usage).unwrap();
        // (200 * 0.14 + 800 * 0.014 + 500 * 0.28) / 1M = (28 + 11.2 + 140) / 1M = 0.0001792
        assert!((price - 0.0001792).abs() < 1e-6, "got {price}");
    }

    #[test]
    fn format_usage_includes_cache() {
        let usage = ChatUsage {
            prompt_tokens: 1000,
            completion_tokens: 500,
            total_tokens: 1500,
            cache_hit_tokens: Some(800),
            cache_miss_tokens: None,
        };
        let s = format_usage("deepseek-v4-flash", &usage);
        assert!(s.contains("缓存命中"));
        assert!(s.contains("80.0%"));
        assert!(s.contains("$"));
    }
}

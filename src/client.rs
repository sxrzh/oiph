//! OpenAI 兼容的 LLM 客户端。

use std::time::Duration;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    // 部分供应商（如 deepseek-reasoner）返回的思维链字段，仅用于展示，不回传。
    #[serde(default, rename = "reasoning_content", skip_serializing)]
    pub reasoning: Option<String>,
}

impl Message {
    pub fn system(text: impl Into<String>) -> Self {
        Self {
            role: "system".into(),
            content: Some(text.into()),
            tool_calls: None,
            tool_call_id: None,
            reasoning: None,
        }
    }

    pub fn user(text: impl Into<String>) -> Self {
        Self {
            role: "user".into(),
            content: Some(text.into()),
            tool_calls: None,
            tool_call_id: None,
            reasoning: None,
        }
    }

    pub fn tool(result: String, tool_call_id: String) -> Self {
        Self {
            role: "tool".into(),
            content: Some(result),
            tool_calls: None,
            tool_call_id: Some(tool_call_id),
            reasoning: None,
        }
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
}

#[derive(Debug, Deserialize)]
pub struct ChatResponse {
    pub choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
pub struct Choice {
    pub message: Message,
    #[allow(dead_code)]
    pub finish_reason: Option<String>,
}

pub struct Client {
    http: reqwest::Client,
    base_url: String,
    api_key: String,
}

impl Client {
    pub fn new(base_url: String, api_key: String) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(180))
            .build()
            .context("构造 HTTP 客户端失败")?;
        Ok(Self {
            http,
            base_url,
            api_key,
        })
    }

    pub async fn chat(
        &self,
        model: &str,
        messages: &[Message],
        tools: &[Tool],
    ) -> Result<ChatResponse> {
        let url = format!(
            "{}/chat/completions",
            self.base_url.trim_end_matches('/')
        );
        let body = ChatRequest {
            model,
            messages,
            tools,
            stream: false,
        };

        let resp = self
            .http
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .context("请求模型供应商失败")?;

        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();

        if !status.is_success() {
            anyhow::bail!("模型供应商错误（HTTP {}）：\n{}", status, text);
        }

        serde_json::from_str::<ChatResponse>(&text).with_context(|| {
            format!("解析模型响应失败：\n{}", text)
        })
    }

    /// 调用 OpenAI 兼容 `/embeddings` 端点，分批返回向量。
    pub async fn embeddings(
        &self,
        model: &str,
        inputs: &[String],
    ) -> Result<Vec<Vec<f32>>> {
        const BATCH: usize = 16;
        let url = format!("{}/embeddings", self.base_url.trim_end_matches('/'));
        let mut out = Vec::with_capacity(inputs.len());
        for batch in inputs.chunks(BATCH) {
            #[derive(Serialize)]
            struct EmbedReq<'a> {
                model: &'a str,
                input: &'a [String],
            }
            #[derive(Deserialize)]
            struct EmbedData {
                embedding: Vec<f32>,
                index: usize,
            }
            #[derive(Deserialize)]
            struct EmbedResp {
                data: Vec<EmbedData>,
            }

            let resp = self
                .http
                .post(&url)
                .bearer_auth(&self.api_key)
                .json(&EmbedReq {
                    model,
                    input: batch,
                })
                .send()
                .await
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
            anyhow::ensure!(
                data.len() == batch.len(),
                "embeddings 返回 {} 个向量，期望 {} 个",
                data.len(),
                batch.len()
            );
            out.extend(data.into_iter().map(|d| d.embedding));
        }
        Ok(out)
    }
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
}

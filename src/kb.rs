//! 知识库（RAG）：全局（~/.oiph/kb）与工程（<工程>/.oiph/kb）两级目录。
//!
//! 每个知识库目录内有一个 `kb.json` 存储文件；检索时合并所有目录的分块。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};

use crate::client::Client;

pub const STORE_FILE: &str = "kb.json";
pub const LOCAL_BACKEND: &str = "local-hash-512";
const MAX_CHUNK_CHARS: usize = 900;
const OVERLAP_CHARS: usize = 100;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KbChunk {
    pub source: String,
    pub chunk_id: usize,
    pub text: String,
    pub vector: Vec<f32>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Kb {
    pub backend: String,
    pub dim: usize,
    pub chunks: Vec<KbChunk>,
}

/// 检索配置：多个知识库目录 + 供应商参数。
#[derive(Clone)]
pub struct KbConfig {
    pub dirs: Vec<PathBuf>,
    pub base_url: String,
    pub api_key: String,
    pub embed_model: Option<String>,
}

impl KbConfig {
    pub fn backend(&self) -> String {
        backend_of(self.embed_model.as_deref())
    }
}

pub fn backend_of(embed_model: Option<&str>) -> String {
    match embed_model {
        Some(m) => format!("openai:{m}"),
        None => LOCAL_BACKEND.to_string(),
    }
}

fn store_path(dir: &Path) -> PathBuf {
    dir.join(STORE_FILE)
}

// ---------- chunking ----------

pub fn chunk_text(text: &str) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut cur = String::new();

    for para in text.split("\n\n") {
        let para = para.trim_matches(['\n', ' ', '\t']);
        if para.is_empty() {
            continue;
        }
        let para_chars = para.chars().count();
        if para_chars > MAX_CHUNK_CHARS + MAX_CHUNK_CHARS / 2 {
            if !cur.is_empty() {
                chunks.push(std::mem::take(&mut cur));
            }
            hard_split(para, &mut chunks);
        } else if cur.chars().count() + para_chars < MAX_CHUNK_CHARS {
            if !cur.is_empty() {
                cur.push('\n');
            }
            cur.push_str(para);
        } else {
            chunks.push(std::mem::take(&mut cur));
            cur.push_str(para);
        }
    }
    if !cur.is_empty() {
        chunks.push(cur);
    }
    chunks
}

fn hard_split(para: &str, out: &mut Vec<String>) {
    let chars: Vec<char> = para.chars().collect();
    let step = MAX_CHUNK_CHARS - OVERLAP_CHARS;
    let mut start = 0;
    while start < chars.len() {
        let end = (start + MAX_CHUNK_CHARS).min(chars.len());
        out.push(chars[start..end].iter().collect());
        if end == chars.len() {
            break;
        }
        start += step;
    }
}

// ---------- local hash embedding ----------

fn fnv1a(key: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in key.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn bump(v: &mut [f32], dim: usize, key: &str) {
    let i = (fnv1a(key) % dim as u64) as usize;
    v[i] += 1.0;
}

/// 特征哈希词袋：ascii 单词 + CJK 单字/双字。结果 L2 归一化，故余弦相似度即点积。
pub fn hash_embed(text: &str, dim: usize) -> Vec<f32> {
    let mut v = vec![0.0f32; dim];
    let lower: Vec<char> = text.to_lowercase().chars().collect();

    let mut word = String::new();
    let mut cjk_run: Vec<char> = Vec::new();

    let flush_word = |word: &mut String, v: &mut Vec<f32>| {
        if word.chars().count() > 1 || word.chars().all(|c| c.is_ascii_digit()) {
            bump(v, dim, &format!("w:{word}"));
        }
        word.clear();
    };
    let flush_cjk = |run: &mut Vec<char>, v: &mut Vec<f32>| {
        for c in run.iter() {
            bump(v, dim, &format!("c:{c}"));
        }
        for pair in run.windows(2) {
            let key: String = pair.iter().collect();
            bump(v, dim, &format!("b:{key}"));
        }
        run.clear();
    };

    for ch in lower {
        if ch.is_whitespace() {
            flush_word(&mut word, &mut v);
            flush_cjk(&mut cjk_run, &mut v);
        } else if ch.is_ascii_alphanumeric() {
            flush_cjk(&mut cjk_run, &mut v);
            word.push(ch);
        } else {
            flush_word(&mut word, &mut v);
            cjk_run.push(ch);
        }
    }
    flush_word(&mut word, &mut v);
    flush_cjk(&mut cjk_run, &mut v);

    normalize(&mut v);
    v
}

pub fn normalize(v: &mut [f32]) {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
}

pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

// ---------- 存储 ----------

pub fn load_dir(dir: &Path) -> Result<Option<Kb>> {
    let p = store_path(dir);
    if !p.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&p)
        .with_context(|| format!("读取知识库 '{}' 失败", p.display()))?;
    let kb: Kb = serde_json::from_str(&raw)
        .with_context(|| format!("知识库 '{}' 已损坏", p.display()))?;
    Ok(Some(kb))
}

pub fn save_dir(kb: &Kb, dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir)
        .with_context(|| format!("创建 '{}' 失败", dir.display()))?;
    std::fs::write(store_path(dir), serde_json::to_vec_pretty(kb)?)
        .with_context(|| format!("写入知识库 '{}' 失败", store_path(dir).display()))?;
    Ok(())
}

pub fn clear_dir(dir: &Path) -> Result<()> {
    let p = store_path(dir);
    if p.exists() {
        std::fs::remove_file(&p).with_context(|| format!("删除 '{}' 失败", p.display()))?;
    }
    Ok(())
}

// ---------- 添加文档 ----------

async fn embed_texts(
    backend: &str,
    texts: &[String],
    dim: usize,
    base_url: &str,
    api_key: &str,
    embed_model: Option<&str>,
) -> Result<Vec<Vec<f32>>> {
    if backend == LOCAL_BACKEND {
        return Ok(texts.iter().map(|t| hash_embed(t, dim)).collect());
    }
    let model = embed_model.ok_or_else(|| anyhow!("供应商 embeddings 需要 --embedding-model"))?;
    let client = Client::new(base_url.to_string(), api_key.to_string())?;
    client.embeddings(model, texts).await
}

/// 向指定知识库目录添加（或替换）一篇文档，返回新增分块数。
pub async fn add_document(
    dir: &Path,
    source: &str,
    text: &str,
    base_url: &str,
    api_key: &str,
    embed_model: Option<&str>,
) -> Result<usize> {
    let backend = backend_of(embed_model);
    let pieces = chunk_text(text);
    anyhow::ensure!(!pieces.is_empty(), "'{source}' 没有可用的文本内容");

    let dim = backend_dim(&backend);
    let vectors = embed_texts(&backend, &pieces, dim, base_url, api_key, embed_model).await?;

    let mut kb = load_dir(dir)?.unwrap_or(Kb {
        backend: backend.clone(),
        dim,
        chunks: Vec::new(),
    });
    anyhow::ensure!(
        kb.backend == backend,
        "知识库 '{}' 使用后端 '{}'，当前配置使用 '{}'。请用相同 --embedding-model，或先清空。",
        dir.display(),
        kb.backend,
        backend
    );

    kb.chunks.retain(|c| c.source != source);
    let n = pieces.len();
    for (chunk_id, (text, vector)) in pieces.into_iter().zip(vectors).enumerate() {
        kb.chunks.push(KbChunk {
            source: source.to_string(),
            chunk_id,
            text,
            vector,
        });
    }
    save_dir(&kb, dir)?;
    Ok(n)
}

fn display_source(raw: &str) -> String {
    std::fs::canonicalize(raw)
        .map(|p: PathBuf| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| raw.to_string())
}

pub async fn cmd_add(
    source: &str,
    dir: &Path,
    base_url: &str,
    api_key: &str,
    embed_model: Option<&str>,
    label: Option<&str>,
) -> Result<()> {
    let text = std::fs::read_to_string(source)
        .with_context(|| format!("读取文件 '{source}' 失败"))?;
    let source_label = label
        .map(String::from)
        .unwrap_or_else(|| display_source(source));
    let n = add_document(dir, &source_label, &text, base_url, api_key, embed_model).await?;
    println!("已向 {} 添加 {n} 个分块", dir.display());
    Ok(())
}

pub fn cmd_list(dirs: &[PathBuf]) -> Result<()> {
    let mut any = false;
    for dir in dirs {
        match load_dir(dir)? {
            None => continue,
            Some(kb) => {
                any = true;
                let mut by_source: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
                for c in &kb.chunks {
                    let e = by_source.entry(&c.source).or_insert((0, 0));
                    e.0 += 1;
                    e.1 += c.text.chars().count();
                }
                println!(
                    "知识库 {}：后端 {}，{} 个分块",
                    dir.display(),
                    kb.backend,
                    kb.chunks.len()
                );
                for (src, (n, chars)) in by_source {
                    println!("  {src}: {n} 分块，{chars} 字符");
                }
            }
        }
    }
    if !any {
        println!("知识库为空（用 `preparer kb add <文件>` 添加文档）");
    }
    Ok(())
}

pub fn cmd_clear(dir: &Path) -> Result<()> {
    clear_dir(dir)?;
    println!("知识库 '{}' 已清空", dir.display());
    Ok(())
}

// ---------- 检索 ----------

fn backend_dim(backend: &str) -> usize {
    match backend.strip_prefix("local-hash-") {
        Some(n) => n.parse().unwrap_or(512),
        None => 512,
    }
}

pub async fn search(cfg: &KbConfig, query: &str, k: usize) -> Result<String> {
    let backend = cfg.backend();

    let mut all: Vec<KbChunk> = Vec::new();
    let mut total_chunks = 0usize;
    for dir in &cfg.dirs {
        let Some(kb) = load_dir(dir)? else {
            continue;
        };
        anyhow::ensure!(
            kb.backend == backend,
            "知识库 '{}' 使用后端 '{}'，当前配置 '{}'，结果无意义",
            dir.display(),
            kb.backend,
            backend
        );
        total_chunks += kb.chunks.len();
        all.extend(kb.chunks);
    }
    if all.is_empty() {
        return Ok("知识库为空（尚未添加任何文档）".into());
    }

    let qvec = embed_texts(
        &backend,
        &[query.to_string()],
        backend_dim(&backend),
        &cfg.base_url,
        &cfg.api_key,
        cfg.embed_model.as_deref(),
    )
    .await?
    .into_iter()
    .next()
    .ok_or_else(|| anyhow!("embedding 未返回向量"))?;

    let mut scored: Vec<(f32, KbChunk)> = all
        .into_iter()
        .map(|c| (cosine(&c.vector, &qvec), c))
        .collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    let k = k.clamp(1, 8);
    let mut out = String::from("知识库检索结果：\n\n");
    out.push_str(&format!("（共 {total_chunks} 个分块）\n\n"));
    for (i, (score, chunk)) in scored.into_iter().take(k).enumerate() {
        out.push_str(&format!(
            "[{}/{}] 相似度 {:.3} | 来源：{}（分块 {}）\n{}\n\n",
            i + 1,
            k,
            score,
            chunk.source,
            chunk.chunk_id,
            clip_chars(&chunk.text, 1200)
        ));
    }
    Ok(out)
}

fn clip_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        s.chars().take(max).collect::<String>() + "...[已截断]"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg_with(dirs: Vec<PathBuf>) -> KbConfig {
        KbConfig {
            dirs,
            base_url: "http://localhost:1/v1".into(),
            api_key: "test".into(),
            embed_model: None,
        }
    }

    #[test]
    fn fnv1a_is_deterministic() {
        assert_eq!(fnv1a("hello"), fnv1a("hello"));
        assert_ne!(fnv1a("hello"), fnv1a("hellp"));
    }

    #[test]
    fn hash_embed_same_text_is_self_similar() {
        let a = hash_embed("The quick brown fox", 512);
        let b = hash_embed("The quick brown fox", 512);
        assert!((cosine(&a, &b) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn hash_embed_matches_cjk_query() {
        let doc = hash_embed("Rust 是一门注重内存安全的系统编程语言", 512);
        let query = hash_embed("内存安全 编程语言", 512);
        let other = hash_embed("chocolate cake recipe", 512);
        assert!(cosine(&doc, &query) > cosine(&doc, &other));
    }

    #[tokio::test]
    async fn search_missing_dirs_is_empty() {
        let dir = std::env::temp_dir().join("preparer-test-does-not-exist");
        let out = search(&cfg_with(vec![dir]), "anything", 1).await.unwrap();
        assert!(out.contains("为空"), "got: {out}");
    }

    #[tokio::test]
    async fn add_search_clear_roundtrip() {
        let dir = std::env::temp_dir().join(format!("prep_kb_{}", uuid::Uuid::new_v4()));
        let _ = clear_dir(&dir);

        let n = add_document(
            &dir,
            "doc1",
            "The Eiffel Tower is located in Paris.\n\nRust is a systems programming language.",
            "http://localhost:1/v1",
            "test",
            None,
        )
        .await
        .unwrap();
        // 两段较短文本合并为一个分块
        assert_eq!(n, 1);

        let cfg = cfg_with(vec![dir.clone()]);
        let out = search(&cfg, "Where is the Eiffel Tower?", 2).await.unwrap();
        assert!(out.contains("Paris"), "got: {out}");
        assert!(out.contains("相似度"), "got: {out}");

        clear_dir(&dir).unwrap();
        assert!(search(&cfg, "x", 1).await.unwrap().contains("为空"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn search_merges_two_dirs() {
        let d1 = std::env::temp_dir().join(format!("prep_kb1_{}", uuid::Uuid::new_v4()));
        let d2 = std::env::temp_dir().join(format!("prep_kb2_{}", uuid::Uuid::new_v4()));
        add_document(&d1, "a", "Eiffel Tower is in Paris.", "http://localhost:1/v1", "test", None)
            .await
            .unwrap();
        add_document(&d2, "b", "Rust is a systems language.", "http://localhost:1/v1", "test", None)
            .await
            .unwrap();
        let out = search(&cfg_with(vec![d1.clone(), d2.clone()]), "Paris", 2)
            .await
            .unwrap();
        assert!(out.contains("Paris"), "got: {out}");
        assert!(out.contains("Eiffel Tower"), "got: {out}");
        std::fs::remove_dir_all(&d1).ok();
        std::fs::remove_dir_all(&d2).ok();
    }
}

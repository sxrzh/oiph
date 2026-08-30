//! Skills：全局（~/.oiph/skills）与工程（<工程>/.oiph/skills）两级目录。
//!
//! 一个 skill 是一个子目录，内含 `SKILL.md`，文件以 YAML frontmatter 开头：
//!
//! ```text
//! ---
//! name: rust-code-review
//! description: Review Rust code ...
//! ---
//!
//! # 正文指令
//! ```
//!
//! name 与 description 会嵌入各 agent 的系统提示词；需要执行时由 agent 通过
//! `load_skill` 工具读取全文。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

pub const SKILL_FILE: &str = "SKILL.md";
const MAX_SKILL_CHARS: usize = 20000;

#[derive(Debug, Clone)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub path: PathBuf,
}

#[derive(Debug, Deserialize)]
struct Frontmatter {
    name: Option<String>,
    description: Option<String>,
}

fn split_frontmatter(content: &str) -> (Option<Frontmatter>, &str) {
    let rest = match content.strip_prefix("---\n") {
        Some(r) => r,
        None => return (None, content),
    };
    let Some(end) = rest.find("\n---") else {
        return (None, content);
    };
    let yaml = &rest[..end];
    let after = rest[end..].strip_prefix("\n---").unwrap_or("");
    let after = after.strip_prefix('\n').unwrap_or(after);
    match serde_yaml::from_str::<Frontmatter>(yaml) {
        Ok(fm) => (Some(fm), after),
        Err(_) => (None, content),
    }
}

fn first_meaningful_line(body: &str) -> String {
    body.lines()
        .map(|l| l.trim())
        .find(|l| !l.is_empty() && *l != "---")
        .unwrap_or("")
        .trim_start_matches('#')
        .trim()
        .to_string()
}

fn parse_skill_file(path: &Path) -> Result<Skill> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("读取 skill 文件失败：{}", path.display()))?;
    let (fm, body) = split_frontmatter(&content);
    let dir_name = path
        .parent()
        .and_then(|p| p.file_name())
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let name = fm
        .as_ref()
        .and_then(|f| f.name.clone())
        .filter(|n| !n.is_empty())
        .unwrap_or(dir_name);
    let description = fm
        .as_ref()
        .and_then(|f| f.description.clone())
        .filter(|d| !d.is_empty())
        .unwrap_or_else(|| first_meaningful_line(body));
    Ok(Skill {
        name,
        description,
        path: path.to_path_buf(),
    })
}

/// 扫描多个根目录发现 skills。同名的以后面的根目录（工程）覆盖先前的（全局）。
pub fn discover(roots: &[PathBuf]) -> Vec<Skill> {
    let mut map: BTreeMap<String, Skill> = BTreeMap::new();
    for root in roots {
        let Ok(rd) = std::fs::read_dir(root) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if !p.is_dir() {
                continue;
            }
            let f = p.join(SKILL_FILE);
            if !f.is_file() {
                continue;
            }
            if let Ok(skill) = parse_skill_file(&f) {
                map.insert(skill.name.clone(), skill);
            }
        }
    }
    map.into_values().collect()
}

/// 按名称加载 skill 全文（工程优先于全局）。
pub fn load_content(roots: &[PathBuf], name: &str) -> Result<String> {
    if name.contains('/') || name.contains("..") || name.is_empty() {
        bail!("skill 名称非法");
    }
    for root in roots.iter().rev() {
        let f = root.join(name).join(SKILL_FILE);
        if f.is_file() {
            let content = std::fs::read_to_string(&f)
                .with_context(|| format!("读取 skill '{name}' 失败"))?;
            return Ok(truncate(&content));
        }
    }
    // 兜底：按 frontmatter 名匹配（目录名与 skill 名不同的情况）
    for skill in discover(roots) {
        if skill.name == name {
            let content = std::fs::read_to_string(&skill.path)
                .with_context(|| format!("读取 skill '{name}' 失败"))?;
            return Ok(truncate(&content));
        }
    }
    bail!(
        "未找到 skill '{name}'（已扫描：{}）",
        roots.iter().map(|r| r.display().to_string()).collect::<Vec<_>>().join("，")
    )
}

fn truncate(s: &str) -> String {
    if s.chars().count() > MAX_SKILL_CHARS {
        let cut: String = s.chars().take(MAX_SKILL_CHARS).collect();
        format!("{cut}...[已截断]")
    } else {
        s.to_string()
    }
}

/// 把内置 skills 写入全局目录（已存在则跳过），返回写入数量。
pub fn ensure_builtin(dir: &Path, builtin: &[(&str, &str)]) -> Result<usize> {
    let mut n = 0;
    for (name, content) in builtin {
        let f = dir.join(name).join(SKILL_FILE);
        if f.exists() {
            continue;
        }
        std::fs::create_dir_all(f.parent().expect("skill 路径有父目录"))?;
        std::fs::write(&f, content)
            .with_context(|| format!("写入内置 skill '{name}' 失败"))?;
        n += 1;
    }
    Ok(n)
}

/// 生成嵌入系统提示词的 skills 清单段。空列表返回空串。
pub fn prompt_section(skills: &[Skill]) -> String {
    if skills.is_empty() {
        return String::new();
    }
    let mut s = String::from(
        "\n\n## 可用 Skills\n\
以下 skills 可用，需要执行相关任务时先调用 load_skill 加载全文并遵循其指令：\n",
    );
    for sk in skills {
        s.push_str(&format!("- {}: {}\n", sk.name, sk.description));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
---
name: rust-code-review
description: Review Rust code for correctness.
---

# Rust Code Review

## Workflow

1. Inspect the crate.
";

    fn write_skill(root: &Path, dir: &str, content: &str) {
        std::fs::create_dir_all(root.join(dir)).unwrap();
        std::fs::write(root.join(dir).join(SKILL_FILE), content).unwrap();
    }

    #[test]
    fn parse_frontmatter_and_fallbacks() {
        let d = std::env::temp_dir().join(format!("prep_skill_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();

        // 标准 frontmatter
        write_skill(&d, "a", SAMPLE);
        let skills = discover(&[d.clone()]);
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "rust-code-review");
        assert_eq!(skills[0].description, "Review Rust code for correctness.");

        // 无 frontmatter：名称用目录名，描述用首个正文行
        write_skill(&d, "b", "# 对拍\n\n内容");
        let skills = discover(&[d.clone()]);
        assert_eq!(skills.len(), 2);
        let b = skills.iter().find(|s| s.name == "b").unwrap();
        assert_eq!(b.description, "对拍");

        // 加载全文
        let full = load_content(&[d.clone()], "rust-code-review").unwrap();
        assert!(full.contains("Workflow"));
        assert!(load_content(&[d.clone()], "nope").is_err());

        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn project_overrides_global() {
        let g = std::env::temp_dir().join(format!("prep_sg_{}", uuid::Uuid::new_v4()));
        let p = std::env::temp_dir().join(format!("prep_sp_{}", uuid::Uuid::new_v4()));
        write_skill(&g, "x", "---\nname: x\ndescription: global\n---\nbody");
        write_skill(&p, "x", "---\nname: x\ndescription: project\n---\nbody");
        let skills = discover(&[g.clone(), p.clone()]);
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].description, "project");
        std::fs::remove_dir_all(&g).ok();
        std::fs::remove_dir_all(&p).ok();
    }

    #[test]
    fn ensure_builtin_writes_once() {
        let d = std::env::temp_dir().join(format!("prep_bi_{}", uuid::Uuid::new_v4()));
        let docs = [("demo", "---\nname: demo\ndescription: d\n---\nhi")];
        assert_eq!(ensure_builtin(&d, &docs).unwrap(), 1);
        assert_eq!(ensure_builtin(&d, &docs).unwrap(), 0);
        assert!(d.join("demo").join(SKILL_FILE).is_file());
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn prompt_section_lists_skills() {
        let skills = vec![Skill {
            name: "duipai".into(),
            description: "对拍".into(),
            path: PathBuf::new(),
        }];
        let s = prompt_section(&skills);
        assert!(s.contains("duipai: 对拍"));
        assert!(prompt_section(&[]).is_empty());
    }
}

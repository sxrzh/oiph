//! `oiph init`：初始化 `~/.oiph`（原 init.sh 的功能集成）。
//!
//! - 安装 skills（整目录）与 prompts，默认跳过已存在项，`--force` 强制覆盖
//! - 构建全局知识库（assets/kb 递归，来源标签 `<builtin>/<相对路径>`）
//! - 安装 vendor（testlib.h / testlib_lemon.h）
//! - 生成 limit.json 与 agents.json（仅当不存在时，`--force` 也不覆盖）

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::paths;

/// 解析资产来源目录：--assets > ./assets > <可执行文件目录>/assets。
fn resolve_assets_dir(explicit: Option<&str>) -> Result<PathBuf> {
    if let Some(d) = explicit {
        let p = PathBuf::from(d);
        anyhow::ensure!(p.is_dir(), "资产目录不存在：{}", p.display());
        return Ok(p);
    }
    let mut candidates: Vec<PathBuf> = vec![PathBuf::from("assets")];
    if let Some(exe) = std::env::current_exe().ok()
        && let Some(parent) = exe.parent() {
            candidates.push(parent.join("assets"));
        }
    for c in &candidates {
        if c.is_dir() {
            return Ok(c.clone());
        }
    }
    bail!(
        "未找到资产目录（尝试 ./assets 与可执行文件旁的 assets）。\
请用 --assets <目录> 指定包含 skills/ kb/ prompts/ auxiliary/ lemon/ 的资产目录"
    )
}

/// 递归收集目录下所有文件（相对路径）。
fn collect_files(dir: &Path, base: &Path, out: &mut Vec<(PathBuf, String)>) -> Result<()> {
    for e in std::fs::read_dir(dir).with_context(|| format!("读取 {}", dir.display()))? {
        let e = e?;
        let p = e.path();
        let rel = p
            .strip_prefix(base)
            .unwrap_or(&p)
            .to_string_lossy()
            .into_owned();
        if p.is_dir() {
            collect_files(&p, base, out)?;
        } else {
            out.push((p.clone(), rel));
        }
    }
    Ok(())
}

fn copy_dir_contents(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    for e in std::fs::read_dir(src)? {
        let e = e?;
        let p = e.path();
        let target = dst.join(e.file_name());
        if p.is_dir() {
            copy_dir_contents(&p, &target)?;
        } else {
            std::fs::copy(&p, &target)?;
        }
    }
    Ok(())
}

/// 执行初始化。返回无。
pub async fn run_init(force: bool, assets: Option<&str>) -> Result<()> {
    let assets_dir = resolve_assets_dir(assets)?;
    let home = paths::oiph_home();
    std::fs::create_dir_all(&home)?;
    println!("✓ {}（来源：{}）", home.display(), assets_dir.display());

    // 1. skills（整目录；已存在跳过，--force 覆盖）
    let skills_src = assets_dir.join("skills");
    if skills_src.is_dir() {
        let skills_dst = paths::global_skills_dir();
        for e in std::fs::read_dir(&skills_src)? {
            let e = e?;
            let src = e.path();
            if !src.is_dir() {
                continue;
            }
            let name = e.file_name().to_string_lossy().into_owned();
            let dst = skills_dst.join(&name);
            if dst.exists() && !force {
                println!("  skill {name} 已存在，跳过（--force 覆盖）");
                continue;
            }
            if dst.exists() {
                std::fs::remove_dir_all(&dst)?;
            }
            copy_dir_contents(&src, &dst)?;
            println!("✓ 安装 skill {name}");
        }
    } else {
        eprintln!("警告：{} 不存在", skills_src.display());
    }

    // 2. 全局知识库（kb add -g，来源标签 <builtin>/<rel>；重复添加按来源覆盖）
    let kb_src = assets_dir.join("kb");
    if kb_src.is_dir() {
        let mut docs = Vec::new();
        collect_files(&kb_src, &kb_src, &mut docs)?;
        docs.sort();
        for (path, rel) in docs {
            let src = path.to_string_lossy().into_owned();
            if let Err(e) = crate::kb::cmd_add(
                &src,
                &paths::global_kb_dir(),
                "", // 本地哈希 embedding，无需 API
                "",
                None,
                Some(rel.as_str()),
            )
            .await
            {
                eprintln!("⚠ 知识库文档 {rel} 添加失败：{e:#}");
            }
        }
    } else {
        eprintln!("警告：{} 不存在", kb_src.display());
    }

    // 3. prompts（已存在跳过，--force 覆盖）
    let prompts_src = assets_dir.join("prompts");
    if prompts_src.is_dir() {
        let prompts_dst = crate::config::config_dir().join("prompts");
        std::fs::create_dir_all(&prompts_dst)?;
        for e in std::fs::read_dir(&prompts_src)? {
            let e = e?;
            let p = e.path();
            if !p.is_file() {
                continue;
            }
            let name = e.file_name().to_string_lossy().into_owned();
            let dst = prompts_dst.join(&name);
            if dst.exists() && !force {
                println!("  prompt {name} 已存在，跳过（--force 覆盖）");
                continue;
            }
            std::fs::copy(&p, &dst)?;
            println!("✓ 安装 prompt {name}");
        }
    } else {
        eprintln!("警告：{} 不存在", prompts_src.display());
    }

    // 4. vendor（testlib.h / testlib_lemon.h；已存在跳过，--force 覆盖）
    let vendor_dst = paths::vendor_dir();
    let vendor_pairs = [
        (assets_dir.join("vendor/testlib.h"), "testlib.h"),
        (assets_dir.join("vendor/testlib_lemon.h"), "testlib_lemon.h"),
    ];
    if vendor_pairs.iter().all(|(s, _)| s.is_file()) {
        std::fs::create_dir_all(&vendor_dst)?;
        for (src, name) in vendor_pairs {
            let dst = vendor_dst.join(name);
            if dst.exists() && !force {
                println!("  vendor {name} 已存在，跳过（--force 覆盖）");
                continue;
            }
            std::fs::copy(&src, &dst)?;
            println!("✓ 安装 vendor {name}");
        }
    } else {
        eprintln!(
            "警告：缺少 {} 或 {}",
            vendor_pairs[0].0.display(),
            vendor_pairs[1].0.display()
        );
    }

    // 5. limit.json（不存在才生成，--force 也不覆盖）
    let limit_file = crate::config::config_dir().join("limit.json");
    if !limit_file.exists() {
        std::fs::create_dir_all(crate::config::config_dir())?;
        std::fs::write(
            &limit_file,
            "{ \"limit_fee\": { \"limit\": 100.0, \"used\": 0.0, \"warn\": 10.0, \"currency\": \"CNY\" } }\n",
        )?;
        println!("✓ 生成 {}（费用预算 100 CNY，剩余低于 10 时告警）", limit_file.display());
    } else {
        println!("  limit.json 已存在，跳过");
    }

    // 6. agents.json（不存在才生成，--force 也不覆盖）
    let agents_file = crate::config::agents_config_path();
    if !agents_file.exists() {
        std::fs::create_dir_all(crate::config::config_dir())?;
        let prompts_dir = crate::config::config_dir().join("prompts");
        let prompt = |name: &str| prompts_dir.join(format!("{name}.md")).display().to_string();
        let cfg = serde_json::json!({
            "supervisor": { "base_url": null, "api_key": null, "prompt": prompt("supervisor") },
            "statement":  { "base_url": null, "api_key": null, "prompt": prompt("statement") },
            "solution":   { "base_url": null, "api_key": null, "prompt": prompt("solution") },
            "auxiliary":  { "base_url": null, "api_key": null, "prompt": prompt("auxiliary") },
            "searching":  { "base_url": null, "api_key": null, "prompt": prompt("searching") },
            "compactor":  { "base_url": null, "api_key": null, "prompt": prompt("compactor") }
        });
        std::fs::write(&agents_file, serde_json::to_vec_pretty(&cfg)?)?;
        println!("✓ 生成 {}", agents_file.display());
    } else {
        println!("  agents.json 已存在，跳过");
    }

    // 7. 前端（~/.oiph/frontend/dist；已存在跳过，--force 覆盖）
    //    来源依次尝试：<assets>/frontend/dist、./frontend/dist、<exe>/frontend/dist
    install_frontend(&assets_dir, force)?;

    println!("初始化完成。全局配置目录：{}", home.display());
    Ok(())
}

/// 安装 Web 前端到 `~/.oiph/frontend/dist`。
fn install_frontend(assets_dir: &Path, force: bool) -> Result<()> {
    let web_dst = paths::web_dist_dir();
    let mut candidates: Vec<PathBuf> = vec![
        assets_dir.join("frontend").join("dist"),
        PathBuf::from("frontend").join("dist"),
    ];
    if let Some(exe) = std::env::current_exe().ok()
        && let Some(parent) = exe.parent() {
            candidates.push(parent.join("frontend").join("dist"));
        }
    let Some(src) = candidates.iter().find(|p| p.is_dir()) else {
        eprintln!(
            "警告：未找到 frontend/dist（已尝试 ./frontend/dist 等），页面将不可用。\
请构建前端后手动复制到 {}",
            web_dst.display()
        );
        return Ok(());
    };
    if web_dst.exists() && !force {
        println!("  前端已存在（{}），跳过（--force 覆盖）", web_dst.display());
        return Ok(());
    }
    if web_dst.exists() {
        std::fs::remove_dir_all(&web_dst)?;
    }
    copy_dir_contents(src, &web_dst)?;
    println!("✓ 安装前端（来源 {}）", src.display());
    Ok(())
}

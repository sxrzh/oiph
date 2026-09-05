//! 编译期内置资产：testlib.h、常见 checker 模板、内置知识库文档与 skills。
//!
//! - `get_testlib` / `get_checker` 工具直接把文件写到工程目录。
//! - 内置知识库文档（assets/kb/*）在启动时种子到全局知识库 ~/.oiph/kb。
//! - 内置 skills（assets/skills/*/SKILL.md）在启动时种子到 ~/.oiph/skills。
//!
//! KB_DOCS 和 BUILTIN_SKILLS 由 build.rs 自动扫描 assets/kb/ 和 assets/skills/
//! 生成，添加文件无需修改代码。
//!
//! testlib.h 不再嵌入二进制：由 init.sh 安装到 ~/.oiph/vendor/，运行时从
//! vendor 读取（见 paths::vendor_read）。

pub const CHECKER_NAMES: &[&str] = &[
    "acmp", "caseicmp", "casencmp", "casewcmp", "dcmp", "fcmp", "hcmp",
    "icmp", "lcmp", "ncmp", "nyesno", "pointscmp", "pointsinfo",
    "rcmp", "rcmp4", "rcmp6", "rcmp9", "rncmp", "uncmp", "wcmp", "yesno",
];

pub fn checker_source(name: &str) -> Option<&'static str> {
    match name {
        "acmp" => Some(include_str!("../assets/auxiliary/checkers/acmp.cpp")),
        "caseicmp" => Some(include_str!("../assets/auxiliary/checkers/caseicmp.cpp")),
        "casencmp" => Some(include_str!("../assets/auxiliary/checkers/casencmp.cpp")),
        "casewcmp" => Some(include_str!("../assets/auxiliary/checkers/casewcmp.cpp")),
        "dcmp" => Some(include_str!("../assets/auxiliary/checkers/dcmp.cpp")),
        "fcmp" => Some(include_str!("../assets/auxiliary/checkers/fcmp.cpp")),
        "hcmp" => Some(include_str!("../assets/auxiliary/checkers/hcmp.cpp")),
        "icmp" => Some(include_str!("../assets/auxiliary/checkers/icmp.cpp")),
        "lcmp" => Some(include_str!("../assets/auxiliary/checkers/lcmp.cpp")),
        "ncmp" => Some(include_str!("../assets/auxiliary/checkers/ncmp.cpp")),
        "nyesno" => Some(include_str!("../assets/auxiliary/checkers/nyesno.cpp")),
        "pointscmp" => Some(include_str!("../assets/auxiliary/checkers/pointscmp.cpp")),
        "pointsinfo" => Some(include_str!("../assets/auxiliary/checkers/pointsinfo.cpp")),
        "rcmp" => Some(include_str!("../assets/auxiliary/checkers/rcmp.cpp")),
        "rcmp4" => Some(include_str!("../assets/auxiliary/checkers/rcmp4.cpp")),
        "rcmp6" => Some(include_str!("../assets/auxiliary/checkers/rcmp6.cpp")),
        "rcmp9" => Some(include_str!("../assets/auxiliary/checkers/rcmp9.cpp")),
        "rncmp" => Some(include_str!("../assets/auxiliary/checkers/rncmp.cpp")),
        "uncmp" => Some(include_str!("../assets/auxiliary/checkers/uncmp.cpp")),
        "wcmp" => Some(include_str!("../assets/auxiliary/checkers/wcmp.cpp")),
        "yesno" => Some(include_str!("../assets/auxiliary/checkers/yesno.cpp")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_checkers_resolvable() {
        for name in CHECKER_NAMES {
            assert!(checker_source(name).is_some(), "缺失 checker: {name}");
            assert!(!checker_source(name).unwrap().is_empty());
        }
    }
}

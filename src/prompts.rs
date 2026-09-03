//! 各角色 Agent 的系统提示词。
//!
//! 提示词不硬编码：默认文件在 assets/prompts/，由 init.sh 安装到
//! `~/.oiph/config/prompts/<agent>.md`，运行时从 `~/.oiph/config/agents.json`
//! 指定的路径加载（见 config.rs）。

use crate::agent::Role;

/// 五个 agent 的系统提示词。
#[derive(Debug, Clone, Default)]
pub struct AgentPrompts {
    pub supervisor: String,
    pub statement: String,
    pub solution: String,
    pub auxiliary: String,
    pub searching: String,
}

impl AgentPrompts {
    pub fn get(&self, role: Role) -> &str {
        match role {
            Role::Supervisor => &self.supervisor,
            Role::Statement => &self.statement,
            Role::Solution => &self.solution,
            Role::Auxiliary => &self.auxiliary,
            Role::Searching => &self.searching,
        }
    }

    pub fn set(&mut self, role: Role, text: String) {
        match role {
            Role::Supervisor => self.supervisor = text,
            Role::Statement => self.statement = text,
            Role::Solution => self.solution = text,
            Role::Auxiliary => self.auxiliary = text,
            Role::Searching => self.searching = text,
        }
    }
}

pub fn role_name(role: Role) -> &'static str {
    match role {
        Role::Supervisor => "supervisor",
        Role::Searching => "searching",
        Role::Statement => "statement",
        Role::Solution => "solution",
        Role::Auxiliary => "auxiliary",
    }
}

pub fn role_from_name(name: &str) -> Option<Role> {
    match name {
        "supervisor" => Some(Role::Supervisor),
        "searching" => Some(Role::Searching),
        "statement" => Some(Role::Statement),
        "solution" => Some(Role::Solution),
        "auxiliary" => Some(Role::Auxiliary),
        _ => None,
    }
}

/// 子 Agent 结束时需在最后一行输出 RESULT 标志，supervisor 据此更新组件状态。
pub const RESULT_HINT: &str = "\n\n## 完成标志\n\
结束时，最后单独一行输出以下之一（供 supervisor 解析状态）：\n\
- `RESULT: OK`\n\
- `RESULT: FAILED: <失败原因>`";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_prompts_get_set() {
        let mut p = AgentPrompts::default();
        assert_eq!(p.get(Role::Supervisor), "");
        p.set(Role::Supervisor, "test".into());
        assert_eq!(p.get(Role::Supervisor), "test");
        assert_eq!(p.get(Role::Statement), "");
        assert_eq!(role_from_name("solution"), Some(Role::Solution));
        assert_eq!(role_from_name("nope"), None);
    }

    #[test]
    fn each_role_has_name() {
        for role in [
            Role::Supervisor,
            Role::Searching,
            Role::Statement,
            Role::Solution,
            Role::Auxiliary,
        ] {
            assert!(!role_name(role).is_empty());
        }
    }
}

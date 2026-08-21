use super::traits::AgentInfo;
use std::path::PathBuf;
use std::process::Command;

pub(crate) fn detect_agents() -> Vec<AgentInfo> {
    vec![detect_codex(), detect_claude(), detect_cursor()]
}

fn detect_codex() -> AgentInfo {
    let (path, version, available) = probe_binary("codex");
    AgentInfo {
        name: "codex".into(),
        version,
        path,
        available,
        capabilities: if available {
            vec![
                "non-interactive".into(),
                "json-output".into(),
                "approve-mode".into(),
            ]
        } else {
            vec![]
        },
    }
}

fn detect_claude() -> AgentInfo {
    let (path, version, available) = probe_binary("claude");
    AgentInfo {
        name: "claude-code".into(),
        version,
        path,
        available,
        capabilities: if available {
            vec!["non-interactive".into(), "json-output".into(), "mcp".into()]
        } else {
            vec![]
        },
    }
}

fn detect_cursor() -> AgentInfo {
    let (path, version, available) = probe_binary("cursor");
    AgentInfo {
        name: "cursor".into(),
        version,
        path,
        available,
        capabilities: if available {
            vec!["acp".into(), "cloud-agents".into()]
        } else {
            vec![]
        },
    }
}

fn probe_binary(name: &str) -> (PathBuf, Option<String>, bool) {
    match which_binary(name) {
        Some(p) => {
            let v = get_version(&p);
            (p, v, true)
        }
        None => (PathBuf::from(name), None, false),
    }
}

fn which_binary(name: &str) -> Option<PathBuf> {
    Command::new("which")
        .arg(name)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| {
            let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if s.is_empty() {
                None
            } else {
                Some(PathBuf::from(s))
            }
        })
}

fn get_version(path: &PathBuf) -> Option<String> {
    Command::new(path)
        .arg("--version")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .trim()
                .lines()
                .next()
                .unwrap_or("")
                .to_string()
        })
        .filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_returns_three_agents() {
        let agents = detect_agents();
        assert_eq!(agents.len(), 3);
        assert_eq!(agents[0].name, "codex");
        assert_eq!(agents[1].name, "claude-code");
        assert_eq!(agents[2].name, "cursor");
    }
}

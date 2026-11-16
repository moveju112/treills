use serde_json::Value;
use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;

#[derive(Clone, Debug, Default)]
pub struct Session {
    pub id: String,
    pub source: String,
    pub cwd: String,
    pub model: String,
    pub phase: String,
    pub last_epoch: u64,
    pub file: PathBuf,
    pub tools: BTreeMap<String, usize>,
    pub skills: BTreeMap<String, usize>,
    pub agents: BTreeMap<String, String>,
    pub activity: VecDeque<String>,
    pub ended: bool,
}

impl Session {
    pub fn new(id: &str, source: &str) -> Self {
        Self { id: id.to_owned(), source: source.to_owned(), phase: "unknown".into(), ..Self::default() }
    }
    fn add(&mut self, line: String) {
        if self.activity.len() >= 35 { self.activity.pop_front(); }
        self.activity.push_back(line);
    }
    fn tool(&mut self, name: &str) {
        if name.is_empty() { return; }
        *self.tools.entry(name.to_owned()).or_default() += 1;
        self.add(format!("tool  {name}"));
    }
    fn skill(&mut self, name: &str, confirmed: bool) {
        let label = if confirmed { format!("{name} [confirmed]") } else { format!("{name} [candidate]") };
        *self.skills.entry(label.clone()).or_default() += 1;
        self.add(format!("skill {label}"));
    }
    pub fn apply(&mut self, obj: &Value) {
        if let Some(cwd) = obj.get("cwd").and_then(Value::as_str) { self.cwd = cwd.to_owned(); }
        if let Some(ts) = obj.get("ts").or_else(|| obj.get("timestamp")).and_then(Value::as_str) {
            let date = ts.get(0..19).unwrap_or(ts);
            self.add(format!("at    {date}"));
        }
        if let Some(model) = obj.pointer("/message/model").or_else(|| obj.get("model")).and_then(Value::as_str) {
            self.model = model.to_owned();
        }
        if let Some(event) = obj.get("event").or_else(|| obj.get("ev")).and_then(Value::as_str) {
            match event {
                "SessionEnd" | "session_end" => { self.ended = true; self.phase = "ended".into(); self.add("session ended".into()); }
                "SessionStart" | "session_start" => { self.phase = "started".into(); self.add("session started".into()); }
                "SubagentStart" => {
                    let id = obj.get("agent_id").and_then(Value::as_str).unwrap_or("unknown");
                    let kind = obj.get("agent_type").and_then(Value::as_str).unwrap_or("agent");
                    self.agents.insert(id.into(),kind.into());
                    self.add(format!("agent +{kind} ({id})"));
                }
                "SubagentStop" => {
                    let id = obj.get("agent_id").and_then(Value::as_str).unwrap_or("unknown");
                    let kind = self.agents.remove(id).unwrap_or_else(|| "agent".into());
                    self.add(format!("agent -{kind} ({id})"));
                }
                "PreToolUse" | "PostToolUse" | "tool" => {
                    if let Some(tool) = obj.get("tool").or_else(|| obj.get("tool_name")).and_then(Value::as_str) {
                        self.tool(tool);
                    }
                }
                "Skill" | "skill" => if let Some(name) = obj.get("skill").and_then(Value::as_str) { self.skill(name,true); },
                _ => {}
            }
        }
        if let Some(skill) = obj.get("skill_name").and_then(Value::as_str) { self.skill(skill,true); }
        if let Some(t) = obj.get("type").and_then(Value::as_str) {
            match t {
                "assistant" => {
                    if let Some(content) = obj.pointer("/message/content").and_then(Value::as_array) {
                        for item in content {
                            let ty = item.get("type").and_then(Value::as_str).unwrap_or("");
                            if ty == "tool_use" {
                                let name = item.get("name").and_then(Value::as_str).unwrap_or("");
                                self.tool(name);
                                if name == "Read" {
                                    if let Some(path) = item.pointer("/input/file_path").and_then(Value::as_str) {
                                        let p = std::path::Path::new(path);
                                        if p.file_name().and_then(|x|x.to_str()) == Some("SKILL.md") {
                                            if let Some(skill) = p.parent().and_then(|x|x.file_name()).and_then(|x|x.to_str()) {
                                                self.skill(skill, false);
                                            }
                                        }
                                    }
                                }
                                if name == "Skill" {
                                    if let Some(skill) = item.pointer("/input/skill").and_then(Value::as_str) { self.skill(skill,true); }
                                }
                                if name == "Agent" || name == "Task" {
                                    let kind = item.pointer("/input/subagent_type").and_then(Value::as_str).unwrap_or("agent");
                                    self.add(format!("agent dispatch {kind}"));
                                }
                            }
                        }
                    }
                }
                "event_msg" | "response_item" => {
                    let p = obj.get("payload").unwrap_or(obj);
                    let ty = p.get("type").and_then(Value::as_str).unwrap_or("");
                    if ty == "exec_command_begin" || ty == "exec_command_end" { self.tool("exec_command"); }
                    if ty == "function_call" {
                        if let Some(n) = p.get("name").and_then(Value::as_str) { self.tool(n); }
                    }
                }
                _ => {}
            }
        }
        if let Some(payload) = obj.get("payload") {
            if let Some(name) = payload.get("name").and_then(Value::as_str) {
                if payload.get("type").and_then(Value::as_str) == Some("function_call") { self.tool(name); }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tracks_tools_and_confirmed_skill() {
        let mut s=Session::new("1","claude");
        s.apply(&serde_json::json!({"type":"assistant","message":{"model":"opus","content":[{"type":"tool_use","name":"Skill","input":{"skill":"review"}},{"type":"tool_use","name":"Read"}]}}));
        assert_eq!(s.model,"opus");
        assert_eq!(s.tools.get("Read"),Some(&1));
        assert_eq!(s.skills.get("review [confirmed]"),Some(&1));
    }
    #[test]
    fn agent_lifecycle() {
        let mut s=Session::new("1","claude");
        s.apply(&serde_json::json!({"event":"SubagentStart","agent_id":"a","agent_type":"explorer"}));
        assert_eq!(s.agents.len(),1);
        s.apply(&serde_json::json!({"event":"SubagentStop","agent_id":"a"}));
        assert!(s.agents.is_empty());
    }
}

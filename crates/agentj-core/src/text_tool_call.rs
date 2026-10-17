//! Recovery for models that write their tool call as **plain text** instead of using the
//! structured tool-call channel.
//!
//! Observed live with opencode-go (`gemini`-free / `minimax`-style upstreams): the assistant
//! text arrives as
//!
//! ```text
//! analysis <|constrain|>xml<|message|><rfl:parameter name="command">
//! ls docs/changelog docs/guide 2>/dev/null; echo "---"; ls crates
//! </rfl:parameter>
//! ```
//!
//! with `finish_reason: "stop"` and **no** `tool_calls`. The gateway never translated the
//! markup, so the runner used to accept it as the turn's final answer: the tool never ran and
//! the task stopped without a conclusion ("继续" could not help either, the history had no
//! tool round-trip).
//!
//! [`recover_tool_call`] turns such markup back into a real tool call when the parameter name
//! pins the tool down unambiguously; [`looks_like_tool_call_text`] lets the runner refuse to
//! treat markup it cannot parse as an answer. @since 0.29.10

use serde_json::{Value, json};

/// A tool call recovered from assistant text.
#[derive(Debug, Clone, PartialEq)]
pub struct RecoveredToolCall {
    /// Tool the text asked for (`bash`, `read`, `glob`, `grep`).
    pub tool: String,
    /// Arguments object, e.g. `{"command": "ls -la"}`.
    pub arguments: Value,
}

/// Markers that mean "this text is a tool call, not an answer".
const MARKERS: [&str; 5] = [
    "<rfl:parameter",
    "</rfl:parameter>",
    "<|constrain|>",
    "<tool_call>",
    "<function_call>",
];

/// `<rfl:parameter name="…">` dialect: parameter name → (tool, argument key).
fn map_parameter(name: &str) -> Option<(&'static str, &'static str)> {
    match name {
        "command" | "cmd" => Some(("bash", "command")),
        "path" | "file_path" => Some(("read", "path")),
        "pattern" | "glob" => Some(("glob", "pattern")),
        "regex" | "query" => Some(("grep", "pattern")),
        _ => None,
    }
}

/// True when `text` carries tool-call markup — such text must never be shown as the answer.
pub fn looks_like_tool_call_text(text: &str) -> bool {
    MARKERS.iter().any(|marker| text.contains(marker))
}

/// Recover the tool call a textual dialect encodes, when the mapping is unambiguous.
pub fn recover_tool_call(text: &str) -> Option<RecoveredToolCall> {
    let blocks = parameter_blocks(text);
    if blocks.len() == 1 {
        let (name, body) = blocks[0];
        let (tool, key) = map_parameter(name)?;
        let body = body.trim();
        if body.is_empty() {
            return None;
        }
        return Some(RecoveredToolCall {
            tool: tool.to_string(),
            arguments: json!({ key: body }),
        });
    }
    recover_json_tool_call(text)
}

/// Every complete `<rfl:parameter name="X">body</rfl:parameter>` block, in order.
fn parameter_blocks(text: &str) -> Vec<(&str, &str)> {
    const OPEN: &str = "<rfl:parameter";
    const CLOSE: &str = "</rfl:parameter>";
    let mut blocks = vec![];
    let mut rest = text;
    while let Some(start) = rest.find(OPEN) {
        let after = &rest[start + OPEN.len()..];
        let Some(gt) = after.find('>') else { break };
        let Some(name) = attr_value(&after[..gt], "name") else {
            rest = &after[gt + 1..];
            continue;
        };
        let body = &after[gt + 1..];
        let Some(end) = body.find(CLOSE) else { break };
        blocks.push((name, &body[..end]));
        rest = &body[end + CLOSE.len()..];
    }
    blocks
}

/// `name="bash"` / `name='bash'` inside an opening tag's attribute string.
fn attr_value<'a>(attrs: &'a str, key: &str) -> Option<&'a str> {
    let pattern = format!("{key}=");
    let idx = attrs.find(&pattern)? + pattern.len();
    let rest = attrs[idx..].trim_start();
    let quote = rest.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let rest = &rest[quote.len_utf8()..];
    Some(&rest[..rest.find(quote)?])
}

/// Hermes/Qwen style `<tool_call>{"name":"bash","arguments":{…}}</tool_call>`.
fn recover_json_tool_call(text: &str) -> Option<RecoveredToolCall> {
    let (open, close) = if text.contains("<tool_call>") {
        ("<tool_call>", "</tool_call>")
    } else {
        ("<function_call>", "</function_call>")
    };
    let start = text.find(open)? + open.len();
    let end = text[start..].find(close)? + start;
    let payload: Value = serde_json::from_str(text[start..end].trim()).ok()?;
    let tool = payload.get("name")?.as_str()?.trim().to_string();
    let arguments = match payload.get("arguments")?.clone() {
        Value::String(raw) => serde_json::from_str(&raw).ok()?,
        other => other,
    };
    if tool.is_empty() || !arguments.is_object() {
        return None;
    }
    Some(RecoveredToolCall { tool, arguments })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The live leak from the user's session (`69e0d7a0-…json`), verbatim.
    const LIVE_LEAK: &str = "analysis <|constrain|>xml<|message|><rfl:parameter name=\"command\">\nls docs/changelog docs/guide 2>/dev/null; echo \"---\"; ls crates; echo \"--- git remotes ---\"; git remote -v; echo \"--- status ---\"; git status --short | head\n</rfl:parameter>";

    #[test]
    fn recovers_the_live_leak_as_a_bash_call() {
        assert!(looks_like_tool_call_text(LIVE_LEAK));
        let call = recover_tool_call(LIVE_LEAK).expect("live leak must be recoverable");
        assert_eq!(call.tool, "bash");
        assert_eq!(
            call.arguments["command"],
            "ls docs/changelog docs/guide 2>/dev/null; echo \"---\"; ls crates; echo \"--- git remotes ---\"; git remote -v; echo \"--- status ---\"; git status --short | head"
        );
    }

    #[test]
    fn recovers_a_lone_closing_fragment_as_markup_only() {
        // The earlier "passed total: 922\n</rfl:parameter>" symptom: recognisable, not parseable.
        let fragment = "passed total: 922\n</rfl:parameter>";
        assert!(looks_like_tool_call_text(fragment));
        assert!(recover_tool_call(fragment).is_none());
    }

    #[test]
    fn maps_the_other_parameter_names() {
        let read =
            recover_tool_call("<rfl:parameter name='path'>src/main.rs</rfl:parameter>").unwrap();
        assert_eq!(read.tool, "read");
        assert_eq!(read.arguments["path"], "src/main.rs");
        let glob =
            recover_tool_call("<rfl:parameter name=\"pattern\">**/*.rs</rfl:parameter>").unwrap();
        assert_eq!(glob.tool, "glob");
        assert_eq!(glob.arguments["pattern"], "**/*.rs");
    }

    #[test]
    fn an_unknown_parameter_is_not_guessed() {
        assert!(recover_tool_call("<rfl:parameter name=\"mystery\">x</rfl:parameter>").is_none());
    }

    #[test]
    fn multiple_parameters_are_not_guessed() {
        let two = "<rfl:parameter name=\"command\">ls</rfl:parameter>\
                   <rfl:parameter name=\"path\">a.rs</rfl:parameter>";
        assert!(recover_tool_call(two).is_none());
    }

    #[test]
    fn recovers_the_json_dialect() {
        let call = recover_tool_call(
            "<tool_call>{\"name\": \"bash\", \"arguments\": {\"command\": \"ls\"}}</tool_call>",
        )
        .unwrap();
        assert_eq!(call.tool, "bash");
        assert_eq!(call.arguments["command"], "ls");
        let stringly = recover_tool_call(
            "<tool_call>{\"name\":\"read\",\"arguments\":\"{\\\"path\\\":\\\"a\\\"}\"}</tool_call>",
        )
        .unwrap();
        assert_eq!(stringly.tool, "read");
        assert_eq!(stringly.arguments["path"], "a");
        assert!(recover_tool_call("<tool_call>not json</tool_call>").is_none());
    }

    #[test]
    fn plain_prose_is_not_markup() {
        let prose = "这个项目结构清晰，但 ipc/lib.rs 太长了。";
        assert!(!looks_like_tool_call_text(prose));
        assert!(recover_tool_call(prose).is_none());
    }
}

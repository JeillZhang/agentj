//! `ask` tool — bridges the model's `ask` tool call into the blocking
//! `QuestionModule` flow (design 决策 #2, 数据流 C).
//!
//! The tool validates the model-facing input (2..=8 options), builds
//! [`AskQuestionParams`], calls [`QuestionModule::ask_blocking`] with the
//! [`ToolCallContext::session_id`] and returns the terminal outcome as a JSON
//! object `{"question_id","answers","skipped"}`.

use std::sync::Arc;

use agentj_core::error::ToolError;
use agentj_core::module::{Tool, ToolCallContext};
use agentj_protocol::question::{AskOption, AskQuestionParams};
use serde::Deserialize;

use crate::module::{QuestionModule, QuestionOutcome};

/// Model-facing `ask` tool: presents 2–8 options to the session owner and
/// blocks until they answer, skip, time out or the run is cancelled.
pub struct AskTool {
    module: Arc<QuestionModule>,
}

impl AskTool {
    pub fn new(module: Arc<QuestionModule>) -> Self {
        Self { module }
    }

    fn parse(input: serde_json::Value) -> Result<AskToolInput, ToolError> {
        serde_json::from_value(input).map_err(|e| ToolError::ExecutionFailed {
            name: "ask".into(),
            reason: format!("invalid ask input: {e}"),
        })
    }

    fn validate_options(input: &AskToolInput) -> Result<(), ToolError> {
        let count = input.options.len();
        if (2..=8).contains(&count) {
            return Ok(());
        }
        Err(ToolError::ExecutionFailed {
            name: "ask".into(),
            reason: format!("options must contain 2..=8 entries, got {count}"),
        })
    }

    fn result_json(question_id: &str, outcome: &QuestionOutcome) -> serde_json::Value {
        serde_json::json!({
            "question_id": question_id,
            "answers": outcome.answers(),
            "skipped": outcome.is_skipped(),
        })
    }
}

/// Raw tool input; defaults mirror the wire defaults of `AskQuestionParams`
/// (`allow_multiple = false`, `allow_other = true`).
#[derive(Debug, Deserialize)]
struct AskToolInput {
    question: String,
    #[serde(default)]
    header: Option<String>,
    options: Vec<AskToolOption>,
    #[serde(default)]
    allow_multiple: bool,
    #[serde(default = "default_true")]
    allow_other: bool,
    #[serde(default)]
    timeout_s: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct AskToolOption {
    id: String,
    label: String,
    #[serde(default)]
    hint: Option<String>,
}

/// Serde default for `allow_other` (omitted on the tool input means true).
fn default_true() -> bool {
    true
}

impl From<AskToolOption> for AskOption {
    fn from(option: AskToolOption) -> Self {
        AskOption {
            id: option.id,
            label: option.label,
            hint: option.hint,
        }
    }
}

impl Tool for AskTool {
    fn name(&self) -> &str {
        "ask"
    }

    fn description(&self) -> &str {
        "Ask the user a question with 2-8 options and wait for their answer. \
         Returns the selected option ids, or `other:`-prefixed free text when \
         allowed. Settles as skipped on timeout, cancel or when no owner is \
         available."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "question": {
                    "type": "string",
                    "description": "Question text shown to the user"
                },
                "header": {
                    "type": "string",
                    "description": "Optional short header for the question card"
                },
                "options": {
                    "type": "array",
                    "minItems": 2,
                    "maxItems": 8,
                    "items": {
                        "type": "object",
                        "properties": {
                            "id": { "type": "string", "description": "Stable answer id" },
                            "label": { "type": "string", "description": "Display text" },
                            "hint": { "type": "string", "description": "Optional hint" }
                        },
                        "required": ["id", "label"],
                        "additionalProperties": false
                    }
                },
                "allow_multiple": { "type": "boolean", "default": false },
                "allow_other": { "type": "boolean", "default": true },
                "timeout_s": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Timeout in seconds; omitted waits for the module default ([agent] question_timeout_s, 600s; 1..=86400)"
                }
            },
            "required": ["question", "options"],
            "additionalProperties": false
        })
    }

    /// Context-free execution cannot know the session, so the session-scoped
    /// `execute_with_context` path must be used.
    fn execute(&self, _input: serde_json::Value) -> Result<serde_json::Value, ToolError> {
        Err(ToolError::ExecutionFailed {
            name: "ask".into(),
            reason: "ask requires a session context (execute_with_context)".into(),
        })
    }

    fn execute_with_context(
        &self,
        input: serde_json::Value,
        ctx: &ToolCallContext,
    ) -> Result<serde_json::Value, ToolError> {
        let parsed = Self::parse(input)?;
        Self::validate_options(&parsed)?;
        let params = AskQuestionParams {
            question_id: String::new(), // ask_blocking assigns the real uuid
            session_id: ctx.session_id.clone(),
            question: parsed.question,
            header: parsed.header,
            options: parsed.options.into_iter().map(Into::into).collect(),
            allow_multiple: parsed.allow_multiple,
            allow_other: parsed.allow_other,
            timeout_s: parsed.timeout_s,
        };
        let (question_id, outcome) = self.module.ask_blocking(&ctx.session_id, &params);
        Ok(Self::result_json(&question_id, &outcome))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::module::QuestionModule;
    use agentj_core::error::ToolError;
    use agentj_core::module::{Tool, ToolCallContext};
    use agentj_protocol::question::AskQuestionParams;
    use serde_json::json;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    fn test_ctx() -> ToolCallContext {
        ToolCallContext {
            session_id: "s1".into(),
            workspace_root: PathBuf::from("."),
        }
    }

    fn valid_input() -> serde_json::Value {
        json!({
            "question": "Pick a letter",
            "options": [
                { "id": "a", "label": "A" },
                { "id": "b", "label": "B" }
            ]
        })
    }

    /// The ask result must be a JSON object (not a JSON-encoded string):
    /// `TurnRunner` uses a string result verbatim and serializes anything else.
    fn ask_json(out: &serde_json::Value) -> serde_json::Value {
        assert!(
            out.is_object(),
            "ask result must be a JSON object, got {out}"
        );
        out.clone()
    }

    #[test]
    fn ask_tool_requires_two_options() {
        let tool = AskTool::new(Arc::new(QuestionModule::new()));
        let one = json!({
            "question": "Pick a letter",
            "options": [{ "id": "a", "label": "A" }]
        });
        let err = tool.execute_with_context(one, &test_ctx()).unwrap_err();
        assert!(
            matches!(&err, ToolError::ExecutionFailed { name, reason }
                if name == "ask" && reason.contains("2..=8")),
            "1 option must be rejected, got {err:?}"
        );
        let nine = json!({
            "question": "Pick a letter",
            "options": [{"id": "a", "label": "A"}, {"id": "b", "label": "B"},
                        {"id": "c", "label": "C"}, {"id": "d", "label": "D"},
                        {"id": "e", "label": "E"}, {"id": "f", "label": "F"},
                        {"id": "g", "label": "G"}, {"id": "h", "label": "H"},
                        {"id": "i", "label": "I"}]
        });
        let err = tool.execute_with_context(nine, &test_ctx()).unwrap_err();
        assert!(
            matches!(&err, ToolError::ExecutionFailed { name, reason }
                if name == "ask" && reason.contains("2..=8")),
            "9 options must be rejected, got {err:?}"
        );
    }

    #[test]
    fn ask_tool_returns_answers_json() {
        let module = Arc::new(QuestionModule::new());
        let seen = Arc::new(Mutex::new(None::<(String, String)>));
        let responder = Arc::clone(&module);
        let seen2 = Arc::clone(&seen);
        module.set_notifier(Arc::new(move |p: &AskQuestionParams, session_id: &str| {
            *seen2.lock().unwrap() = Some((session_id.to_string(), p.question_id.clone()));
            let question_id = p.question_id.clone();
            let module = Arc::clone(&responder);
            std::thread::spawn(move || {
                let _ = module.respond(&question_id, vec!["a".to_string()], false);
            });
            true
        }));
        let tool = AskTool::new(module);
        let out = tool
            .execute_with_context(valid_input(), &test_ctx())
            .unwrap();
        let v = ask_json(&out);
        let question_id = v["question_id"].as_str().unwrap_or_default();
        assert!(
            !question_id.is_empty(),
            "question_id must be the generated uuid: {v}"
        );
        assert_eq!(v["answers"], json!(["a"]));
        assert_eq!(v["skipped"], false);
        let seen = seen.lock().unwrap();
        let (session_id, delivered_id) = seen.as_ref().expect("notifier must fire");
        assert_eq!(
            session_id, "s1",
            "notifier must receive the ToolCallContext session id"
        );
        assert_eq!(
            delivered_id, question_id,
            "delivered question_id must match the tool result"
        );
    }

    #[test]
    fn ask_tool_returns_skipped_without_owner() {
        let module = Arc::new(QuestionModule::new());
        module.set_notifier(Arc::new(|_p: &AskQuestionParams, _sid: &str| false));
        let tool = AskTool::new(module);
        let out = tool
            .execute_with_context(valid_input(), &test_ctx())
            .unwrap();
        let v = ask_json(&out);
        assert!(!v["question_id"].as_str().unwrap_or_default().is_empty());
        assert_eq!(v["answers"], json!([]));
        assert_eq!(v["skipped"], true);
    }

    #[test]
    fn ask_tool_forwards_optional_fields_to_wire() {
        let module = Arc::new(QuestionModule::new());
        let seen = Arc::new(Mutex::new(None::<(String, AskQuestionParams)>));
        let seen2 = Arc::clone(&seen);
        module.set_notifier(Arc::new(move |p: &AskQuestionParams, session_id: &str| {
            *seen2.lock().unwrap() = Some((session_id.to_string(), p.clone()));
            false // no owner: settle skipped immediately, no blocking
        }));
        let tool = AskTool::new(module);
        let input = json!({
            "question": "Pick a letter",
            "header": "Choose",
            "options": [{"id": "a", "label": "A", "hint": "first"}, {"id": "b", "label": "B"}],
            "allow_multiple": true,
            "allow_other": false,
            "timeout_s": 7
        });
        let _ = tool.execute_with_context(input, &test_ctx()).unwrap();
        let seen = seen.lock().unwrap();
        let (session_id, params) = seen.as_ref().expect("notifier must fire");
        assert_eq!(session_id, "s1");
        assert_eq!(params.session_id, "s1");
        assert_eq!(params.header.as_deref(), Some("Choose"));
        assert!(params.allow_multiple);
        assert!(!params.allow_other);
        assert_eq!(params.timeout_s, Some(7));
        assert_eq!(params.options.len(), 2);
        assert_eq!(params.options[0].hint.as_deref(), Some("first"));
    }
}

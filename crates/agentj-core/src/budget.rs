//! Outgoing-request token budget (design §上下文预算).
//!
//! Nothing upstream of a provider knows a model's real context window — the catalog
//! carries no per-model limit — so the kernel keeps a conservative budget of its own for
//! every assembled request, and an upstream context-window rejection tightens it further
//! (see `turn_runner`). Without this the whole session history (and every in-run tool
//! result) was forwarded verbatim until the provider answered
//! `[context_length_exceeded] ... resulted in 3904559 tokens`.

use crate::types::{ChatMessage, Role};

/// Budget used when `[model] max_context_tokens` is unset. @since 0.29.7
pub const DEFAULT_CONTEXT_TOKENS: usize = 128_000;
/// Clamp for any configured/parsed budget: below this a request is useless, above it we
/// are past every window in the catalog. @since 0.29.7
pub const MIN_CONTEXT_TOKENS: usize = 1_000;
pub const MAX_CONTEXT_TOKENS: usize = 10_000_000;
/// Retries allowed after an upstream context-window rejection. @since 0.29.7
pub const MAX_OVERFLOW_RETRIES: usize = 2;

/// Notice appended to a truncated **tool result**. Deliberately distinct from the
/// token-budget marker in `truncate` (`budget.rs:123`): the token path must stay
/// byte-identical, and a model-facing tool result needs an actionable hint. @since 0.29.7
pub const TOOL_RESULT_TRUNCATION_NOTICE: &str =
    "\n…[已截断 {dropped} 字符，原文 {total} 字符；请缩小查询范围]…\n";

/// Approximate tokens: ASCII at ~4 chars/token, everything else at ~1 char/token (CJK is
/// close to one token per character, and under-counting is what breaks the request).
/// @since 0.29.7
pub fn estimate_tokens(text: &str) -> usize {
    let (ascii, wide) = text.chars().fold((0usize, 0usize), |(a, w), c| {
        if c.is_ascii() { (a + 1, w) } else { (a, w + 1) }
    });
    ascii.div_ceil(4) + wide
}

/// Tokens one assembled message costs, tool-call payloads included. @since 0.29.7
pub fn message_tokens(message: &ChatMessage) -> usize {
    let calls: usize = message
        .tool_calls
        .iter()
        .map(|call| estimate_tokens(&call.name) + estimate_tokens(&call.arguments.to_string()) + 8)
        .sum();
    estimate_tokens(&message.content) + calls + 4
}

/// Tokens an assembled request costs. @since 0.29.7
pub fn request_tokens(messages: &[ChatMessage]) -> usize {
    messages.iter().map(message_tokens).sum()
}

/// Clamp a configured or upstream-reported budget into the supported range. @since 0.29.7
pub fn clamp_budget(budget_tokens: usize) -> usize {
    budget_tokens.clamp(MIN_CONTEXT_TOKENS, MAX_CONTEXT_TOKENS)
}

/// Tighten `budget` after an upstream rejection: keep 90% of the reported limit when it is
/// the smaller of the two (head-room for tokenizer drift), else 90% of the current budget.
/// Returns `None` when the budget cannot shrink any further — a retry would resend the
/// identical request, so the caller reports the upstream error instead. @since 0.29.7
pub fn retry_budget(budget: usize, upstream_max: usize) -> Option<usize> {
    let reported = upstream_max.saturating_mul(9) / 10;
    let candidate = if reported > 0 && reported < budget {
        reported
    } else {
        budget.saturating_mul(9) / 10
    };
    let clamped = clamp_budget(candidate);
    (clamped < budget).then_some(clamped)
}

/// Trim `messages` so the request fits `budget_tokens`: keep the system prompt and the
/// newest messages, truncate individual oversized bodies, and never leave a tool result
/// behind without the assistant call that produced it. @since 0.29.7
pub fn fit_messages(messages: Vec<ChatMessage>, budget_tokens: usize) -> Vec<ChatMessage> {
    let budget = clamp_budget(budget_tokens);
    let per_message = budget / 4;
    let prepared: Vec<ChatMessage> = messages
        .into_iter()
        .map(|m| truncate(m, per_message))
        .collect();
    let (system, rest) = match prepared.split_first() {
        Some((first, tail)) if first.role == Role::System => (Some(first.clone()), tail.to_vec()),
        _ => (None, prepared),
    };
    let mut total = request_tokens(system.as_slice()) + request_tokens(&rest);
    let mut drop = 0usize;
    while drop + 1 < rest.len() && total > budget {
        total -= message_tokens(&rest[drop]);
        drop += 1;
    }
    // A prefix cut must not orphan a tool result: providers reject an unpaired one.
    while drop < rest.len() && rest[drop].role == Role::Tool {
        drop += 1;
    }
    let mut fitted = Vec::with_capacity(rest.len() - drop + 1);
    fitted.extend(system);
    fitted.extend(rest.into_iter().skip(drop));
    fitted
}

/// Keep the head and tail of an oversized body, marking what was dropped. System prompts
/// are never touched; tool-call payloads that alone exceed the cap are replaced by a
/// marker so a message can shrink even when its bulk lives in `tool_calls.arguments`.
/// @since 0.29.7
fn truncate(mut message: ChatMessage, cap: usize) -> ChatMessage {
    if message.role == Role::System {
        return message;
    }
    shrink_calls(&mut message, cap);
    let tokens = message_tokens(&message);
    if tokens <= cap {
        return message;
    }
    let total = message.content.chars().count();
    let keep = (total.saturating_mul(cap) / tokens.max(1)).max(2);
    if keep >= total {
        return message;
    }
    let half = keep / 2;
    let head = &message.content[..byte_offset(&message.content, half)];
    let tail = &message.content[byte_offset(&message.content, total - half)..];
    message.content = format!("{head}\n…[已截断 {} 字符]…\n{tail}", total - keep);
    message
}

/// Replace a tool-call payload that alone exceeds `cap` tokens with a marker. @since 0.29.7
fn shrink_calls(message: &mut ChatMessage, cap: usize) {
    let max_chars = cap.saturating_mul(4);
    for call in &mut message.tool_calls {
        let text = call.arguments.to_string();
        let count = text.chars().count();
        if count > max_chars {
            call.arguments =
                serde_json::json!({ "_truncated": format!("…[已截断 {count} 字符]…") });
        }
    }
}

/// Byte offset of the `n`-th char — slicing must stay on a UTF-8 boundary. @since 0.29.7
fn byte_offset(text: &str, n: usize) -> usize {
    text.char_indices().nth(n).map_or(text.len(), |(i, _)| i)
}

/// Truncates on a `char` boundary and returns the head plus a notice.
/// `max_chars` counts **characters of the kept head**, not bytes. @since 0.29.7
pub fn truncate_chars(text: &str, max_chars: usize) -> (String, usize) {
    let total = text.chars().count();
    if total <= max_chars {
        return (text.to_string(), 0);
    }
    let dropped = total - max_chars;
    let head: String = text.chars().take(max_chars).collect();
    let notice = TOOL_RESULT_TRUNCATION_NOTICE
        .replace("{dropped}", &dropped.to_string())
        .replace("{total}", &total.to_string());
    (format!("{head}{notice}"), dropped)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ToolCall;

    #[test]
    fn estimate_tokens_charges_cjk_per_character() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("中文文本"), 4);
        assert_eq!(estimate_tokens("ab中文"), 3);
    }

    #[test]
    fn fit_messages_keeps_system_and_newest_messages() {
        let mut messages = vec![ChatMessage::system("sys")];
        messages.extend((0..20).map(|i| ChatMessage::user(format!("m{i}: {}", "x".repeat(400)))));
        let fitted = fit_messages(messages, MIN_CONTEXT_TOKENS);
        assert_eq!(fitted[0].role, Role::System);
        assert!(fitted.len() < 21, "old history must be dropped");
        assert!(request_tokens(&fitted) <= MIN_CONTEXT_TOKENS);
        assert!(fitted.last().unwrap().content.starts_with("m19"));
    }

    #[test]
    fn fit_messages_truncates_a_single_oversized_body() {
        let big = format!("{}{}", "a".repeat(200_000), "中文".repeat(100));
        let fitted = fit_messages(
            vec![ChatMessage::user("hi"), ChatMessage::user(big)],
            MIN_CONTEXT_TOKENS,
        );
        assert_eq!(fitted.len(), 2);
        assert!(fitted[1].content.contains("已截断"));
        assert!(request_tokens(&fitted) <= MIN_CONTEXT_TOKENS);
    }

    #[test]
    fn fit_messages_never_orphans_a_tool_result() {
        let mut messages = vec![ChatMessage::system("sys")];
        for i in 0..8 {
            messages.push(ChatMessage {
                role: Role::Assistant,
                content: String::new(),
                tool_call_id: None,
                tool_calls: vec![ToolCall {
                    id: format!("c{i}"),
                    name: "bash".into(),
                    arguments: serde_json::json!({}),
                }],
            });
            messages.push(ChatMessage::tool_result(format!("c{i}"), "y".repeat(2000)));
        }
        let fitted = fit_messages(messages, MIN_CONTEXT_TOKENS);
        assert_eq!(fitted[0].role, Role::System);
        assert_ne!(
            fitted[1].role,
            Role::Tool,
            "a dropped tool call must take its result with it"
        );
    }

    #[test]
    fn retry_budget_uses_the_upstream_limit_with_headroom() {
        assert_eq!(retry_budget(100_000, 20_000), Some(18_000));
        assert_eq!(retry_budget(100_000, 0), Some(90_000));
    }

    #[test]
    fn retry_budget_always_shrinks_or_gives_up() {
        // The reported upstream window (1M) is *larger* than our budget: the estimator was
        // wrong, but a second identical request is still pointless — shrink by 10%.
        assert_eq!(retry_budget(128_000, 1_048_576), Some(115_200));
        assert_eq!(retry_budget(1_200, 1_048_576), Some(1_080));
        // At the floor the clamp would cancel the shrink: give up instead of retrying.
        assert_eq!(retry_budget(MIN_CONTEXT_TOKENS, 0), None);
        // A reported window below the clamp floor is followed, not 10%-shrunk.
        assert_eq!(retry_budget(2_000, 500), Some(MIN_CONTEXT_TOKENS));
    }

    #[test]
    fn truncate_shrinks_a_tool_call_payload() {
        let fitted = fit_messages(
            vec![ChatMessage {
                role: Role::Assistant,
                content: String::new(),
                tool_call_id: None,
                tool_calls: vec![ToolCall {
                    id: "c0".into(),
                    name: "bash".into(),
                    arguments: serde_json::json!({ "command": "x".repeat(100_000) }),
                }],
            }],
            MIN_CONTEXT_TOKENS,
        );
        assert!(request_tokens(&fitted) <= MIN_CONTEXT_TOKENS);
        assert!(
            fitted[0].tool_calls[0].arguments["_truncated"].is_string(),
            "an oversized tool-call payload must be replaced by a marker"
        );
    }

    #[test]
    fn truncate_chars_reports_dropped_count() {
        let text = "a".repeat(10_000);
        let (out, dropped) = truncate_chars(&text, 4_000);
        assert_eq!(dropped, 6_000);
        // The notice is pinned literally, not rebuilt from the constant under test,
        // so a corrupted notice text cannot stay green on the length assertion alone.
        let expected = format!(
            "{}\n…[已截断 6000 字符，原文 10000 字符；请缩小查询范围]…\n",
            "a".repeat(4_000)
        );
        assert_eq!(out, expected);
    }

    #[test]
    fn truncate_chars_at_zero_cap_drops_everything() {
        let (out, dropped) = truncate_chars("hello", 0);
        assert_eq!(dropped, 5);
        assert_eq!(out, "\n…[已截断 5 字符，原文 5 字符；请缩小查询范围]…\n");
    }

    #[test]
    fn truncate_chars_on_empty_input_is_a_no_op() {
        let (out, dropped) = truncate_chars("", 100);
        assert_eq!((out.as_str(), dropped), ("", 0));
    }

    #[test]
    fn truncate_chars_at_exactly_the_cap_is_a_no_op() {
        let text = "x".repeat(4_000);
        let (out, dropped) = truncate_chars(&text, 4_000);
        assert_eq!((out.as_str(), dropped), (text.as_str(), 0));
    }

    #[test]
    fn truncate_chars_respects_char_boundaries() {
        let text = "中".repeat(5_000) + "\u{1f642}";
        let (out, _) = truncate_chars(&text, 1_000);
        assert!(out.chars().count() > 1_000);
        assert!(out.contains("已截断"));
    }

    #[test]
    fn truncate_chars_is_a_no_op_below_the_cap() {
        let (out, dropped) = truncate_chars("short", 4_000);
        assert_eq!((out.as_str(), dropped), ("short", 0));
    }
}

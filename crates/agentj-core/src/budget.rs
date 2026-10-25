//! Outgoing-request token budget (design §上下文预算).
//!
//! Nothing upstream of a provider knows a model's real context window — the catalog
//! carries no per-model limit — so the kernel keeps a conservative budget of its own for
//! every assembled request, and an upstream context-window rejection tightens it further
//! (see `turn_runner`). Without this the whole session history (and every in-run tool
//! result) was forwarded verbatim until the provider answered
//! `[context_length_exceeded] ... resulted in 3904559 tokens`.

use crate::types::{ChatMessage, Role, ToolDefinition};

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

/// Approximate tokens: ASCII letters/whitespace at ~4 chars/token, dense ASCII punctuation
/// at ~2.5 chars/token, CJK at ~1.25 chars/token, other BMP characters at ~1 char/token and
/// astral characters at ~0.5 char/token. Under-counting is what breaks the request, so every
/// class is charged at least as much as the pre-seam heuristic (ASCII-only 4:1, everything
/// else 1:1) did. @since 0.29.7
pub fn estimate_tokens(text: &str) -> usize {
    HeuristicTokenizer.count_text(text)
}

/// The seam every budget calculation goes through, so a provider-specific tokenizer can be
/// dropped in without touching the trimming logic. @since 0.29.7
pub trait TokenCounter {
    fn count_text(&self, text: &str) -> usize;
}

/// Character-class heuristic used until a real BPE vocabulary is embedded. @since 0.29.7
#[derive(Debug, Default, Clone, Copy)]
pub struct HeuristicTokenizer;

impl TokenCounter for HeuristicTokenizer {
    fn count_text(&self, text: &str) -> usize {
        if text.is_empty() {
            return 0;
        }
        let (mut word, mut punct, mut cjk, mut bmp, mut astral) =
            (0usize, 0usize, 0usize, 0usize, 0usize);
        for c in text.chars() {
            if c.is_ascii() {
                if c.is_ascii_alphanumeric() || c.is_ascii_whitespace() {
                    word += 1;
                } else {
                    punct += 1;
                }
            } else if is_cjk(c) {
                cjk += 1;
            } else if (c as u32) > 0xFFFF {
                astral += 1;
            } else {
                bmp += 1;
            }
        }
        // words 4/token, punctuation 2.5/token, CJK 1.25/token, other BMP 1/token,
        // astral 0.5/token (`astral * 2`).
        (word.div_ceil(4) + (punct * 2).div_ceil(5) + (cjk * 5).div_ceil(4) + bmp + astral * 2)
            .max(1)
    }
}

/// Han / Hiragana / Katakana / Hangul / CJK compatibility ideographs. @since 0.29.7
fn is_cjk(c: char) -> bool {
    matches!(
        c as u32,
        0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0x3040..=0x30FF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF
    )
}

/// Tokens one assembled message costs, tool-call payloads included. @since 0.29.7
pub fn message_tokens(message: &ChatMessage) -> usize {
    message_tokens_with(message, &HeuristicTokenizer)
}

/// `message_tokens` against an arbitrary counter. @since 0.29.7
pub fn message_tokens_with(message: &ChatMessage, counter: &dyn TokenCounter) -> usize {
    let mut total = counter.count_text(&message.content) + 4;
    if let Some(call) = &message.tool_call_id {
        total += counter.count_text(call);
    }
    for call in &message.tool_calls {
        total +=
            8 + counter.count_text(&call.name) + counter.count_text(&call.arguments.to_string());
    }
    total
}

/// Tokens an assembled request costs. @since 0.29.7
pub fn request_tokens(messages: &[ChatMessage]) -> usize {
    request_tokens_with(messages, &HeuristicTokenizer)
}

/// `request_tokens` against an arbitrary counter. @since 0.29.7
pub fn request_tokens_with(messages: &[ChatMessage], counter: &dyn TokenCounter) -> usize {
    messages
        .iter()
        .map(|m| message_tokens_with(m, counter))
        .sum()
}

/// Per-definition framing cost beyond its name, description and schema. @since 0.29.7
const TOOL_DEF_OVERHEAD: usize = 12;

/// Tokens the tool definitions in a request cost. They are never dropped by the trimmer, but
/// they do occupy the window, so `fit_messages_with` reserves room for them first. @since 0.29.7
pub fn definitions_tokens(defs: &[ToolDefinition]) -> usize {
    defs.iter()
        .map(|d| {
            estimate_tokens(&d.name)
                + estimate_tokens(&d.description)
                + estimate_tokens(&d.input_schema.to_string())
                + TOOL_DEF_OVERHEAD
        })
        .sum()
}

/// Clamp a configured or upstream-reported budget into the supported range. @since 0.29.7
pub fn clamp_budget(budget_tokens: usize) -> usize {
    budget_tokens.clamp(MIN_CONTEXT_TOKENS, MAX_CONTEXT_TOKENS)
}

/// The budget a request is actually trimmed to: the **smallest ceiling wins** across the
/// configured cap and the model's window. A window is only ever trusted at 90% of its
/// advertised size — tokenizer drift and per-message framing are always on our side of the
/// estimate, so aiming at the raw window is how the first request gets rejected. Without a
/// window the cap stands alone; without either we fall back to a conservative default.
/// @since 0.20.0
pub fn effective_budget(cap: Option<usize>, window: Option<usize>) -> usize {
    let by_cap = cap.map(clamp_budget);
    let by_window = window.map(|w| clamp_budget(w * 9 / 10));
    match (by_cap, by_window) {
        (Some(c), Some(w)) => c.min(w),
        (Some(c), None) => c,
        (None, Some(w)) => w,
        (None, None) => DEFAULT_CONTEXT_TOKENS,
    }
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
    fit_messages_with(messages, &[], budget_tokens, &HeuristicTokenizer)
}

/// As `fit_messages`, but first reserves room for the tool definitions the request carries:
/// `defs` never participate in drop-oldest, so they are subtracted from the body budget.
/// A pathological definition set can drive the body budget to 0; the trimmer still keeps the
/// system prompt and the newest message rather than returning an empty request. @since 0.29.7
pub fn fit_messages_with(
    messages: Vec<ChatMessage>,
    defs: &[ToolDefinition],
    budget_tokens: usize,
    counter: &dyn TokenCounter,
) -> Vec<ChatMessage> {
    let budget = clamp_budget(budget_tokens);
    let body = budget.saturating_sub(definitions_tokens(defs));
    fit_messages_body(messages, body, counter)
}

fn fit_messages_body(
    messages: Vec<ChatMessage>,
    budget: usize,
    counter: &dyn TokenCounter,
) -> Vec<ChatMessage> {
    let per_message = budget / 4;
    let prepared: Vec<ChatMessage> = messages
        .into_iter()
        .map(|m| truncate(m, per_message, counter))
        .collect();
    let (system, rest) = match prepared.split_first() {
        Some((first, tail)) if first.role == Role::System => (Some(first.clone()), tail.to_vec()),
        _ => (None, prepared),
    };
    let mut total =
        request_tokens_with(system.as_slice(), counter) + request_tokens_with(&rest, counter);
    let mut drop = 0usize;
    while drop + 1 < rest.len() && total > budget {
        total -= message_tokens_with(&rest[drop], counter);
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
fn truncate(mut message: ChatMessage, cap: usize, counter: &dyn TokenCounter) -> ChatMessage {
    if message.role == Role::System {
        return message;
    }
    shrink_calls(&mut message, cap);
    let tokens = message_tokens_with(&message, counter);
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
    for call in &mut message.tool_calls {
        // Charge the payload with the same counter that will measure it later: a chars-per-token
        // constant cannot cover both ASCII words (4 chars/token) and dense punctuation
        // (2.5) or CJK (1.25), so a char cap either waves an over-budget payload through or
        // throws away payloads that would have fitted.
        let tokens = estimate_tokens(&call.arguments.to_string());
        if tokens > cap {
            call.arguments =
                serde_json::json!({ "_truncated": format!("…[已截断 {tokens} tokens]…") });
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
    use crate::types::{ToolCall, ToolDefinition};

    fn def(name: &str) -> ToolDefinition {
        ToolDefinition {
            name: name.into(),
            description: "x".repeat(400),
            input_schema: serde_json::json!(
                {"type": "object", "properties": {"cmd": {"type": "string"}}}
            ),
        }
    }

    #[test]
    fn definitions_tokens_charges_name_description_and_schema() {
        assert_eq!(definitions_tokens(&[]), 0);
        let one = definitions_tokens(&[def("bash")]);
        let two = definitions_tokens(&[def("bash"), def("read")]);
        assert!(
            one > 100,
            "schema + description must cost real tokens: {one}"
        );
        assert!(two > one);
    }

    #[test]
    fn fit_with_defs_reserves_their_budget() {
        let defs = vec![def("bash"), def("read"), def("write")];
        let messages = || {
            let mut m = vec![ChatMessage::system("sys")];
            m.extend((0..20).map(|i| ChatMessage::user(format!("m{i}: {}", "x".repeat(400)))));
            m
        };
        let bare = fit_messages_with(messages(), &[], MIN_CONTEXT_TOKENS, &HeuristicTokenizer);
        let reserved =
            fit_messages_with(messages(), &defs, MIN_CONTEXT_TOKENS, &HeuristicTokenizer);
        assert!(request_tokens(&reserved) <= request_tokens(&bare));
        assert!(request_tokens(&reserved) + definitions_tokens(&defs) <= MIN_CONTEXT_TOKENS);
        assert_eq!(reserved[0].role, Role::System);
        assert!(reserved.last().unwrap().content.starts_with("m19"));
    }

    #[test]
    fn fit_with_defs_larger_than_the_budget_never_empties_the_request() {
        let defs: Vec<_> = (0..50).map(|i| def(&format!("tool{i}"))).collect();
        let messages = vec![ChatMessage::system("sys"), ChatMessage::user("hello")];
        let fitted = fit_messages_with(messages, &defs, MIN_CONTEXT_TOKENS, &HeuristicTokenizer);
        assert_eq!(
            fitted.len(),
            2,
            "system + newest survive even when defs blow the budget"
        );
        assert_eq!(fitted[0].role, Role::System);
        // body == 0 时 keep = max(2)、half = keep/2 = 1：只剩 1 个头部字符 + 截断标记 + 1 个尾部字符。
        assert!(
            fitted[1].content.starts_with("h\n"),
            "kept: {}",
            fitted[1].content
        );
        assert!(
            fitted[1].content.len() > 2,
            "the truncation marker is kept too"
        );
    }

    #[test]
    fn fit_messages_without_defs_matches_the_plain_entry_point() {
        // ChatMessage 只 derive Debug/Clone/Serialize/Deserialize，没有 PartialEq，
        // 所以比较编码后的大小与条目数，而不是比较值。
        let messages = vec![ChatMessage::system("sys"), ChatMessage::user("hi")];
        let plain = fit_messages(messages.clone(), MIN_CONTEXT_TOKENS);
        let with_defs = fit_messages_with(messages, &[], MIN_CONTEXT_TOKENS, &HeuristicTokenizer);
        assert_eq!(request_tokens(&plain), request_tokens(&with_defs));
        assert_eq!(plain.len(), with_defs.len());
    }

    /// The pre-batch formula, kept in the test module as the lower bound every
    /// input must clear: new estimates may only ever be more conservative.
    fn legacy_estimate(text: &str) -> usize {
        let (ascii, wide) = text.chars().fold((0usize, 0usize), |(a, w), c| {
            if c.is_ascii() { (a + 1, w) } else { (a, w + 1) }
        });
        ascii.div_ceil(4) + wide
    }

    #[test]
    fn heuristic_never_estimates_below_the_legacy_formula() {
        let corpus = [
            "",
            "abcd",
            "The quick brown fox jumps over the lazy dog.",
            r#"{"a": [1, 2, 3], "b": {"c": null}, "d": true}"#,
            "fn main() { let x = vec![1, 2, 3]; println!(\"{:?}\", x); }",
            "中文文本",
            "ab中文",
            "αβγ",
            "café au lait",
            "emoji 🚀🚀 and 中文字 mixed with code `let a = 1;`",
        ];
        for text in corpus {
            assert!(
                estimate_tokens(text) >= legacy_estimate(text),
                "estimate must never be more optimistic than the legacy formula: {text:?}"
            );
        }
        assert_eq!(estimate_tokens(""), 0);
    }

    #[test]
    fn heuristic_charges_dense_punctuation_more_than_prose() {
        let prose = "aaaa bbbb cccc dddd eeee ffff gggg hhhh";
        let punctuation = "{}[]()<>,;:!?@#$%^&*_-+=|~`\"'\\/{}[]()<>,;:";
        assert!(estimate_tokens(punctuation) > estimate_tokens(prose));
    }

    #[test]
    fn heuristic_charges_cjk_above_one_token_per_char() {
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("中文文本"), 5, "CJK costs 1.25 token/char");
        assert_eq!(estimate_tokens("ab中文"), 4);
    }

    #[test]
    fn heuristic_charges_other_bmp_characters_one_token_each() {
        assert_eq!(estimate_tokens("αβγ"), 3);
        assert_eq!(estimate_tokens("café"), 1 + 1, "ascii run + one BMP char");
    }

    #[test]
    fn heuristic_charges_astral_characters_double() {
        assert_eq!(estimate_tokens("🚀🚀"), 4);
        assert!(
            estimate_tokens("🚀🚀") >= legacy_estimate("🚀🚀"),
            "the legacy formula charges 1 per astral char; the new one charges 2"
        );
    }

    #[test]
    fn shrink_calls_measures_the_payload_in_tokens() {
        let big = serde_json::json!({"blob": "y".repeat(40_000)}).to_string();
        let mut message = ChatMessage::user("go");
        message.tool_calls = vec![ToolCall {
            id: "c1".into(),
            name: "bash".into(),
            arguments: serde_json::Value::from(big),
        }];
        shrink_calls(&mut message, 200);
        assert!(
            estimate_tokens(&message.tool_calls[0].arguments.to_string()) <= 200,
            "the shrunk payload must fit the token cap"
        );
        assert!(
            message.tool_calls[0].arguments["_truncated"].is_string(),
            "the truncation marker must survive"
        );
    }

    #[test]
    fn shrink_calls_counts_dense_punctuation_as_tokens_not_chars() {
        // 320 punctuation chars are 320 chars but 128 tokens: a char-based cap of 100 tokens
        // (300 chars) would wave this through and leave the message 28% over budget.
        let blob = "{}[]()<>,;:!?".repeat(20);
        let payload = serde_json::json!({ "q": blob }).to_string();
        assert!(payload.chars().count() <= 300 + 16, "fixture stays under the char cap");
        assert!(estimate_tokens(&payload) > 100, "fixture must exceed the token cap");
        let mut message = ChatMessage::user("go");
        message.tool_calls = vec![ToolCall {
            id: "c1".into(),
            name: "grep".into(),
            arguments: serde_json::Value::from(payload),
        }];
        shrink_calls(&mut message, 100);
        assert!(
            message.tool_calls[0].arguments["_truncated"].is_string(),
            "a payload over the token cap must be shrunk"
        );
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
    fn effective_budget_takes_the_smallest_ceiling() {
        assert_eq!(effective_budget(None, None), DEFAULT_CONTEXT_TOKENS);
        assert_eq!(effective_budget(Some(32_000), None), 32_000);
        assert_eq!(effective_budget(None, Some(200_000)), 180_000);
        assert_eq!(effective_budget(Some(50_000), Some(200_000)), 50_000);
        assert_eq!(effective_budget(Some(500_000), Some(200_000)), 180_000);
        assert_eq!(effective_budget(None, Some(1_000_000)), 900_000);
        assert_eq!(effective_budget(None, Some(10)), MIN_CONTEXT_TOKENS);
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

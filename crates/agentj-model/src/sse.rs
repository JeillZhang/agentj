//! Shared SSE line-stream assembler for wire-format providers.
//!
//! `bytes_stream()` delivers HTTP chunks whose boundaries can split an SSE
//! line mid-JSON (TCP segmentation / TLS record boundaries). Feeding each
//! chunk independently through `.lines()` truncates such lines and fails JSON
//! parsing with "EOF while parsing a string". This module bridges the byte
//! stream to a true line reader (`StreamReader` + `BufReader`, mirroring
//! ollama.rs) so lines are reassembled across chunk boundaries before any
//! parsing happens.

use agentj_core::error::ModelError;
use agentj_core::types::ModelEvent;
use futures::{StreamExt, stream};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio_util::io::StreamReader;

/// Assemble SSE [`ModelEvent`]s from a raw HTTP byte stream.
///
/// `push_line` receives exactly one complete SSE line per call (line ending
/// stripped); it pushes zero or more events into the output vec.
pub(crate) fn sse_event_stream<S, E, F>(
    byte_stream: S,
    push_line: F,
) -> futures::stream::BoxStream<'static, Result<ModelEvent, ModelError>>
where
    S: futures::Stream<Item = Result<bytes::Bytes, E>> + Send + Unpin + 'static,
    E: std::error::Error + Send + Sync + 'static,
    F: FnMut(&str, &mut Vec<Result<ModelEvent, ModelError>>) + Send + 'static,
{
    let mapped = byte_stream.map(|r| r.map_err(std::io::Error::other));
    let reader = BufReader::new(StreamReader::new(mapped));
    stream::unfold(
        (reader, push_line, false),
        |(mut reader, mut push_line, terminated)| async move {
            if terminated {
                return None;
            }
            let mut line = String::new();
            match reader.read_line(&mut line).await {
                Ok(0) => None, // EOF: SSE bodies end with a newline; nothing pending
                Ok(_) => {
                    let trimmed = line.trim_end_matches(['\n', '\r']);
                    let mut events = Vec::new();
                    push_line(trimmed, &mut events);
                    Some((events, (reader, push_line, false)))
                }
                Err(e) => {
                    // Transport error mid-stream: surface once, then end.
                    Some((
                        vec![Err(ModelError::Network(e.to_string()))],
                        (reader, push_line, true),
                    ))
                }
            }
        },
    )
    .flat_map(stream::iter)
    .boxed()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;

    /// Parse `data:` SSE lines of shape {"text":"..."} into Delta events,
    /// `[DONE]` into Done. Mirrors provider parser semantics closely enough
    /// that a truncated line panics on from_str — i.e. reproduces the bug.
    fn json_text_parser(line: &str, out: &mut Vec<Result<ModelEvent, ModelError>>) {
        let line = line.trim();
        let Some(data) = line.strip_prefix("data:") else {
            return;
        };
        let data = data.trim();
        if data == "[DONE]" {
            out.push(Ok(ModelEvent::Done));
            return;
        }
        if data.is_empty() {
            return;
        }
        let v: serde_json::Value = serde_json::from_str(data).unwrap();
        if let Some(text) = v.get("text").and_then(|t| t.as_str()) {
            out.push(Ok(ModelEvent::Delta(text.to_string())));
        }
    }

    fn deltas(events: &[ModelEvent]) -> Vec<String> {
        events
            .iter()
            .filter_map(|e| match e {
                ModelEvent::Delta(t) => Some(t.clone()),
                _ => None,
            })
            .collect()
    }

    async fn collect(chunks: Vec<Result<Bytes, std::io::Error>>) -> Vec<ModelEvent> {
        let stream = sse_event_stream(stream::iter(chunks), json_text_parser);
        let mut events = Vec::new();
        let mut s = Box::pin(stream);
        while let Some(result) = s.next().await {
            match result.unwrap() {
                ModelEvent::Done => break,
                e => events.push(e),
            }
        }
        events
    }

    #[tokio::test]
    async fn line_split_across_chunks_is_reassembled() {
        // One data: line cut mid-string between two chunks — the exact
        // condition that produced "EOF while parsing a string".
        let events = collect(vec![
            Ok(Bytes::from_static(b"data: {\"text\":\"Hel")),
            Ok(Bytes::from_static(b"lo\"}\n\ndata: [DONE]\n\n")),
        ])
        .await;
        assert_eq!(deltas(&events), vec!["Hello"]);
    }

    #[tokio::test]
    async fn multibyte_char_split_across_chunks_survives() {
        // '你' = E4 BD A0; split after its 2nd byte so a per-chunk lossy
        // decode would corrupt the character with U+FFFD.
        let head: &[u8] = b"data: {\"text\":\"\xE4\xBD";
        let tail: &[u8] = b"\xA0\xE5\xA5\xBD\"}\n\ndata: [DONE]\n\n";
        let events = collect(vec![
            Ok(Bytes::from_static(head)),
            Ok(Bytes::from_static(tail)),
        ])
        .await;
        assert_eq!(deltas(&events), vec!["你好"]);
    }

    #[tokio::test]
    async fn crlf_line_endings_supported() {
        let events = collect(vec![Ok(Bytes::from_static(
            b"data: {\"text\":\"hi\"}\r\n\r\ndata: [DONE]\r\n\r\n",
        ))])
        .await;
        assert_eq!(deltas(&events), vec!["hi"]);
    }

    #[tokio::test]
    async fn final_line_without_trailing_newline_is_flushed() {
        let events = collect(vec![Ok(Bytes::from_static(b"data: [DONE]"))]).await;
        assert!(events.is_empty(), "Done terminates collection; no error");
    }

    #[tokio::test]
    async fn transport_error_surfaces_once_then_stream_ends() {
        let stream = sse_event_stream(
            stream::iter(vec![
                Ok::<_, std::io::Error>(Bytes::from_static(b"data: {\"text\":\"ok\"}\n\n")),
                Err(std::io::Error::other("boom")),
            ]),
            json_text_parser,
        );
        let mut s = Box::pin(stream);
        let first = s.next().await.unwrap().unwrap();
        assert_eq!(deltas(&[first]), vec!["ok"]);
        let err = s.next().await.unwrap().unwrap_err();
        assert!(err.to_string().contains("boom"));
        assert!(
            s.next().await.is_none(),
            "stream must terminate after transport error"
        );
    }
}

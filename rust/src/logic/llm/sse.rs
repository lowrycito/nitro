//! Minimal Server-Sent Events parser.
//!
//! Reuses the generic streaming surface so adapters can plug in different
//! backends. We deliberately don't pull in `eventsource-stream` — the SSE
//! flavour the OpenAI/Anthropic APIs emit is the simple "data: line\n\n"
//! one, no retry or comment frames.

use bytes::{Bytes, BytesMut};
use futures::stream::{Stream, StreamExt};

use super::stream::StreamError;

/// One parsed event. Empty-data events ("[DONE]" sentinels are common) are
/// surfaced verbatim — adapters interpret the payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseEvent {
    pub event: Option<String>,
    pub data: String,
}

/// Convert a byte stream from `reqwest::Response::bytes_stream()` into a
/// stream of [`SseEvent`].
pub fn into_sse_events<S>(byte_stream: S) -> impl Stream<Item = Result<SseEvent, StreamError>>
where
    S: Stream<Item = Result<Bytes, reqwest::Error>> + Unpin + Send + 'static,
{
    let mut buf = BytesMut::with_capacity(8 * 1024);
    let mut event_name: Option<String> = None;
    let mut data_lines: Vec<String> = Vec::new();
    let mut byte_stream = byte_stream;

    async_stream::try_stream! {
        while let Some(chunk) = byte_stream.next().await {
            let chunk = chunk?;
            buf.extend_from_slice(&chunk);

            // SSE frames are separated by a blank line. Process complete
            // frames out of the buffer; keep the partial tail.
            while let Some(idx) = find_double_newline(&buf) {
                let frame = buf.split_to(idx + 2);
                let frame = std::str::from_utf8(&frame)
                    .map_err(|e| StreamError::Parse(format!("non-utf8 frame: {e}")))?;

                for line in frame.split('\n') {
                    let line = line.strip_suffix('\r').unwrap_or(line);
                    if line.is_empty() {
                        continue;
                    }
                    if let Some(rest) = line.strip_prefix(':') {
                        // Comment/heartbeat — discard.
                        let _ = rest;
                        continue;
                    }
                    let (field, value) = match line.split_once(':') {
                        Some((f, v)) => (f.trim(), v.strip_prefix(' ').unwrap_or(v)),
                        None => (line, ""),
                    };
                    match field {
                        "event" => event_name = Some(value.to_string()),
                        "data" => data_lines.push(value.to_string()),
                        // `id` and `retry` are SSE features we don't use.
                        _ => {}
                    }
                }

                if !data_lines.is_empty() || event_name.is_some() {
                    let ev = SseEvent {
                        event: event_name.take(),
                        data: data_lines.join("\n"),
                    };
                    data_lines.clear();
                    yield ev;
                }
            }
        }
    }
}

fn find_double_newline(buf: &[u8]) -> Option<usize> {
    // Accept both `\n\n` and `\r\n\r\n`.
    let mut prev_blank = false;
    let mut i = 0;
    while i < buf.len() {
        let b = buf[i];
        if b == b'\n' {
            if prev_blank {
                // We reach the *second* newline. Index of the first newline
                // is i-1 in the LF-only case; for CRLF we returned the
                // position of the second \r\n. We only need a position
                // useful to `split_to`, so return `i-1` to encompass the
                // first newline; the caller adds 2 to step past the blank
                // line marker.
                return Some(i - 1);
            }
            prev_blank = true;
        } else if b == b'\r' {
            // Stay in "we saw newline-ish" state across CRLF.
        } else {
            prev_blank = false;
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream;
    use futures::StreamExt;

    fn run<S>(s: S) -> Vec<Result<SseEvent, StreamError>>
    where
        S: Stream<Item = Result<SseEvent, StreamError>> + Unpin,
    {
        // Helper to exhaust a stream synchronously inside #[tokio::test].
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let mut s = s;
            let mut out = Vec::new();
            while let Some(item) = s.next().await {
                out.push(item);
            }
            out
        })
    }

    fn bytes(s: &'static str) -> Bytes {
        Bytes::from_static(s.as_bytes())
    }

    fn ok_stream(parts: Vec<&'static str>) -> impl Stream<Item = Result<Bytes, reqwest::Error>> {
        stream::iter(parts.into_iter().map(|s| Ok(bytes(s))))
    }

    #[test]
    fn parses_single_event() {
        let s = into_sse_events(ok_stream(vec!["data: hello\n\n"]));
        let mut s = Box::pin(s);
        let out = run(&mut s);
        let evs: Vec<_> = out.into_iter().map(Result::unwrap).collect();
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].data, "hello");
        assert_eq!(evs[0].event, None);
    }

    #[test]
    fn parses_named_event_with_data() {
        let s = into_sse_events(ok_stream(vec![
            "event: message_delta\ndata: {\"a\":1}\n\nevent: ping\n:keepalive\n\n",
        ]));
        let mut s = Box::pin(s);
        let out = run(&mut s);
        let evs: Vec<_> = out.into_iter().map(Result::unwrap).collect();
        assert_eq!(evs.len(), 2);
        assert_eq!(evs[0].event.as_deref(), Some("message_delta"));
        assert_eq!(evs[0].data, "{\"a\":1}");
        assert_eq!(evs[1].event.as_deref(), Some("ping"));
        assert!(evs[1].data.is_empty());
    }

    #[test]
    fn buffers_across_chunk_boundaries() {
        let s = into_sse_events(ok_stream(vec![
            "data: hel",
            "lo wor",
            "ld\n",
            "\nda",
            "ta: two\n\n",
        ]));
        let mut s = Box::pin(s);
        let out = run(&mut s);
        let evs: Vec<_> = out.into_iter().map(Result::unwrap).collect();
        let datas: Vec<_> = evs.iter().map(|e| e.data.as_str()).collect();
        assert_eq!(datas, vec!["hello world", "two"]);
    }

    #[test]
    fn merges_multi_data_lines() {
        let s = into_sse_events(ok_stream(vec!["data: a\ndata: b\n\n"]));
        let mut s = Box::pin(s);
        let out = run(&mut s);
        let evs: Vec<_> = out.into_iter().map(Result::unwrap).collect();
        assert_eq!(evs[0].data, "a\nb");
    }
}

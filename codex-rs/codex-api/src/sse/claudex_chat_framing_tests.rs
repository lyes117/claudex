use super::*;
use pretty_assertions::assert_eq;

#[test]
fn every_split_preserves_utf8_crlf_comments_and_multiline_data() {
    let wire = ": keepalive\r\nid: ignored\r\ndata: é🦀\r\ndata: line two\r\n\r\ndata: [DONE]\n\n"
        .as_bytes();
    for split in 0..=wire.len() {
        let mut framing = ChatSseFramer::new(/*raw_limit*/ 4096, /*frame_limit*/ 1024);
        let mut frames = framing.push(&wire[..split]).unwrap();
        frames.extend(framing.push(&wire[split..]).unwrap());
        assert_eq!(frames, ["é🦀\nline two", "[DONE]"]);
        framing.finish_eof().unwrap();
    }
    let mut framing = ChatSseFramer::new(/*raw_limit*/ 4096, /*frame_limit*/ 1024);
    let frames: Vec<_> = wire
        .iter()
        .flat_map(|byte| framing.push(&[*byte]).unwrap())
        .collect();
    assert_eq!(frames, ["é🦀\nline two", "[DONE]"]);
    framing.finish_eof().unwrap();
}

#[test]
fn comments_and_ignored_fields_spend_frame_and_total_budgets() {
    for wire in [
        b":0123456789".as_slice(),
        b"id:0123456789",
        b"data:0123456789",
    ] {
        let mut framing = ChatSseFramer::new(/*raw_limit*/ 64, /*frame_limit*/ 8);
        assert!(framing.push(wire).is_err());
        assert!(framing.line.len() <= 8);
    }
    let mut framing = ChatSseFramer::new(/*raw_limit*/ 8, /*frame_limit*/ 8);
    framing.push(b":x\n\n").unwrap();
    framing.push(b":x\n\n").unwrap();
    assert!(framing.push(b"\n").is_err());
    let mut framing = ChatSseFramer::new(/*raw_limit*/ 16_384, /*frame_limit*/ 8);
    assert!(framing.push(&vec![b'\n'; 4097]).is_err());
}

#[test]
fn truncated_delimiters_data_and_invalid_utf8_fail_closed() {
    for wire in [b"data: x".as_slice(), b"data: x\n", b": comment", b"\r"] {
        let mut framing = ChatSseFramer::new(/*raw_limit*/ 128, /*frame_limit*/ 128);
        framing.push(wire).unwrap();
        assert!(framing.finish_eof().is_err());
    }
    let mut framing = ChatSseFramer::new(/*raw_limit*/ 128, /*frame_limit*/ 128);
    assert!(framing.push(b"data: \xff\n\n").is_err());
}

#[test]
fn internal_carriage_returns_cannot_hide_comments_fields_or_json_across_any_chunk_split() {
    for wire in [
        b": comment\rdata: [DONE]\n\n".as_slice(),
        b"data: {\"a\":1,\r\"b\":2}\n\n",
        b"id: ignored\rhidden\n\n",
        b"data: [DONE]\r\r\n\r\n",
    ] {
        for split in 0..=wire.len() {
            let mut framing =
                ChatSseFramer::new(/*raw_limit*/ 4096, /*frame_limit*/ 1024);
            let result = framing
                .push(&wire[..split])
                .and_then(|_| framing.push(&wire[split..]));
            assert!(result.is_err());
        }
    }
}

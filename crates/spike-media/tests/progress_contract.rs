use spike_media::{ProgressError, ProgressEvent, ProgressParser};

#[test]
fn parses_complete_progress_blocks_and_ignores_unknown_keys() {
    let input = b"frame=48\nout_time_us=2000000\nspeed=1.25x\nfuture_key=x\nprogress=continue\n";
    let events = ProgressParser::default().push(input).unwrap();
    assert_eq!(
        events,
        vec![ProgressEvent {
            frame: Some(48),
            out_time_us: Some(2_000_000),
            speed: Some(1.25),
            finished: false,
        }]
    );
}

#[test]
fn buffers_split_utf8_and_line_boundaries() {
    let mut parser = ProgressParser::default();
    assert!(parser.push(b"frame=1\nprogr").unwrap().is_empty());
    let events = parser.push(b"ess=end\n").unwrap();
    assert!(events[0].finished);
}

#[test]
fn accepts_unknown_utf8_split_at_an_arbitrary_byte_boundary() {
    let mut parser = ProgressParser::default();
    assert!(parser.push(b"future=\xc3").unwrap().is_empty());
    let events = parser.push(b"\xa9\nprogress=continue\n").unwrap();
    assert_eq!(events.len(), 1);
    assert!(!events[0].finished);
}

#[test]
fn rejects_malformed_known_numeric_values() {
    let error = ProgressParser::default()
        .push(b"frame=not-a-number\nprogress=end\n")
        .unwrap_err();
    assert!(matches!(error, ProgressError::InvalidNumber { key, .. } if key == "frame"));
}

#[test]
fn rejects_pending_input_larger_than_64_kib() {
    let error = ProgressParser::default()
        .push(&vec![b'x'; 65_537])
        .unwrap_err();
    assert!(matches!(error, ProgressError::PendingInputTooLarge { .. }));
}

#[test]
fn rejects_a_current_block_that_grows_past_the_cap_across_chunks() {
    let mut parser = ProgressParser::default();
    let line = vec![b'x'; 32_000];
    parser.push(&[line.as_slice(), b"\n"].concat()).unwrap();
    parser.push(&[line.as_slice(), b"\n"].concat()).unwrap();
    let error = parser.push(&[line.as_slice(), b"\n"].concat()).unwrap_err();
    assert!(matches!(error, ProgressError::BlockTooLarge { .. }));
}

#[test]
fn accepts_a_large_chunk_when_it_contains_many_complete_blocks() {
    let input = b"frame=1\nprogress=continue\n".repeat(4_000);
    assert!(input.len() > 64 * 1024);
    let events = ProgressParser::default().push(&input).unwrap();
    assert_eq!(events.len(), 4_000);
}

#[test]
fn recovers_after_each_malformed_block_error() {
    let failures: [&[u8]; 4] = [
        b"frame=not-a-number\nprogress=end\n",
        b"progress=maybe\n",
        &vec![b'x'; 65_537],
        &[
            vec![b'x'; 32_000],
            vec![b'\n'],
            vec![b'x'; 32_000],
            vec![b'\n'],
            vec![b'x'; 32_000],
            vec![b'\n'],
        ]
        .concat(),
    ];
    for failure in failures {
        let mut parser = ProgressParser::default();
        assert!(parser.push(failure).is_err());
        let events = parser.push(b"frame=2\nprogress=end\n").unwrap();
        assert_eq!(
            events,
            vec![ProgressEvent {
                frame: Some(2),
                out_time_us: None,
                speed: None,
                finished: true
            }]
        );
    }
}

#[test]
fn does_not_emit_incomplete_blocks() {
    let events = ProgressParser::default()
        .push(b"frame=12\nout_time_us=10\n")
        .unwrap();
    assert!(events.is_empty());
}

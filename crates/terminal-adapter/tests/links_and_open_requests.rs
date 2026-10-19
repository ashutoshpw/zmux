use zmux_terminal_adapter::{LinkSpan, Size, Terminal};

const NONCE: &str = "0123456789abcdef0123456789abcdef";
const URL_B64: &str = "aHR0cHM6Ly9leGFtcGxlLmNvbQ";

#[test]
fn visible_plain_links_keep_cell_columns_for_unicode_and_wraps() {
    let mut unicode = Terminal::new(Size { rows: 2, cols: 40 }).unwrap();
    unicode
        .process("x https://example.org/界.".as_bytes())
        .unwrap();
    let viewport = unicode.viewport();
    assert_eq!(
        viewport.links,
        vec![LinkSpan {
            row: 0,
            start_column: 2,
            end_column: 24,
            url: "https://example.org/界".into(),
        }]
    );

    let mut wrapped = Terminal::new(Size { rows: 3, cols: 8 }).unwrap();
    wrapped.process(b"https://x/a").unwrap();
    let viewport = wrapped.viewport();
    assert_eq!(
        viewport.links,
        vec![
            LinkSpan {
                row: 0,
                start_column: 0,
                end_column: 8,
                url: "https://x/a".into(),
            },
            LinkSpan {
                row: 1,
                start_column: 0,
                end_column: 3,
                url: "https://x/a".into(),
            },
        ]
    );

    let delimiter_cases = [
        (
            "https://example.org/path(foo)",
            "https://example.org/path(foo)",
        ),
        (
            "https://example.org/path(one(two))",
            "https://example.org/path(one(two))",
        ),
        (
            "https://example.org/path[bar]",
            "https://example.org/path[bar]",
        ),
        (
            "https://example.org/path{bar}",
            "https://example.org/path{bar}",
        ),
        (
            "https://example.org/path(foo)).",
            "https://example.org/path(foo)",
        ),
        (
            "https://example.org/path[bar]]",
            "https://example.org/path[bar]",
        ),
        (
            "https://example.org/path{bar}}",
            "https://example.org/path{bar}",
        ),
        (
            "https://example.org/path(foo).",
            "https://example.org/path(foo)",
        ),
        (
            "https://example.org/path%28foo%29)",
            "https://example.org/path%28foo%29",
        ),
    ];
    for (input, expected_url) in delimiter_cases {
        let mut terminal = Terminal::new(Size { rows: 1, cols: 80 }).unwrap();
        terminal.process(input.as_bytes()).unwrap();
        let viewport = terminal.viewport();
        assert_eq!(viewport.links.len(), 1, "input: {input}");
        let link = &viewport.links[0];
        assert_eq!(link.url, expected_url, "input: {input}");
        assert_eq!(link.row, 0, "input: {input}");
        assert_eq!(link.start_column, 0, "input: {input}");
        assert_eq!(link.end_column, expected_url.len() as u16, "input: {input}");
    }
}

#[test]
fn viewport_generation_tracks_output_scrolling_and_geometry_changes() {
    let mut terminal = Terminal::new(Size { rows: 2, cols: 8 }).unwrap();
    assert_eq!(terminal.viewport().viewport_generation, 0);
    terminal.process(b"one\r\ntwo\r\nthree").unwrap();
    let output_generation = terminal.viewport().viewport_generation;
    assert_eq!(output_generation, 1);
    assert_eq!(terminal.viewport().viewport_generation, output_generation);

    terminal.resize(Size { rows: 3, cols: 10 }).unwrap();
    assert_eq!(
        terminal.viewport().viewport_generation,
        output_generation + 1
    );
    terminal.scroll(1).unwrap();
    let viewport = terminal.viewport();
    assert_eq!(viewport.viewport_generation, output_generation + 2);
    assert_eq!(viewport.size, Size { rows: 3, cols: 10 });
    assert_eq!(viewport.lines.len(), 3);
    assert_eq!(
        viewport.lines[0].iter().map(|run| run.width).sum::<u16>(),
        10,
        "resized scrollback rows should publish blank cells through the new width"
    );
    assert_eq!(viewport.row_copy.as_ref().unwrap()[0].written_end, 3);
}

#[test]
fn osc8_hyperlinks_are_visible_and_close_at_the_exact_cell_boundary() {
    let mut terminal = Terminal::new(Size { rows: 2, cols: 20 }).unwrap();
    terminal
        .process(b"\x1b]8;;https://osc.example/a\x1b\\click\x1b]8;;\x1b\\")
        .unwrap();
    assert_eq!(
        terminal.viewport().links,
        vec![LinkSpan {
            row: 0,
            start_column: 0,
            end_column: 5,
            url: "https://osc.example/a".into(),
        }]
    );
}

#[test]
fn explicit_open_requests_decode_once_and_keep_the_nonce_private() {
    use zmux_terminal_adapter::MAX_PROGRAM_OPEN_REQUESTS;

    let mut terminal =
        Terminal::new_with_open_request_nonce(Size { rows: 2, cols: 20 }, Some(hex_nonce(NONCE)))
            .unwrap();
    let event = format!("\x1b]777;zmux-open;v1;{NONCE};{URL_B64}\x07");
    for byte in event.as_bytes() {
        terminal.process(&[*byte]).unwrap();
    }
    let mut requests = drain(&mut terminal);
    assert_eq!(requests.len(), 1);
    let request = requests.pop().unwrap();
    assert_eq!(request.url(), "https://example.com");
    assert!(request.matches_nonce(&hex_nonce(NONCE)));
    assert!(!request.matches_nonce(&[0; 16]));
    let serialized_viewport = serde_json::to_string(&terminal.viewport()).unwrap();
    assert!(!serialized_viewport.contains(NONCE));
    assert!(!serialized_viewport.contains("https://example.com"));
    assert!(drain(&mut terminal).is_empty());

    // A parser burst beyond the finite queue is reported to the session owner, never truncated
    // into a partially accepted group of opener effects.
    let burst = event.repeat(MAX_PROGRAM_OPEN_REQUESTS + 1);
    terminal.process(burst.as_bytes()).unwrap();
    assert!(terminal.take_open_requests().is_err());
    assert!(drain(&mut terminal).is_empty());
}

#[test]
fn missing_and_mismatched_nonces_do_not_consume_the_opener_queue() {
    use zmux_terminal_adapter::MAX_PROGRAM_OPEN_REQUESTS;

    const EXPECTED_NONCE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let mismatched_event = format!("\x1b]777;zmux-open;v1;{NONCE};{URL_B64}\x07");

    let mut unregistered = Terminal::new(Size { rows: 2, cols: 20 }).unwrap();
    unregistered
        .process(
            mismatched_event
                .repeat(MAX_PROGRAM_OPEN_REQUESTS + 1)
                .as_bytes(),
        )
        .unwrap();
    assert!(unregistered.take_open_requests().unwrap().is_empty());

    let expected_event = format!("\x1b]777;zmux-open;v1;{EXPECTED_NONCE};{URL_B64}\x07");
    let mut registered = Terminal::new_with_open_request_nonce(
        Size { rows: 2, cols: 20 },
        Some(hex_nonce(EXPECTED_NONCE)),
    )
    .unwrap();
    registered
        .process(
            format!(
                "{}{}",
                mismatched_event.repeat(MAX_PROGRAM_OPEN_REQUESTS + 1),
                expected_event
            )
            .as_bytes(),
        )
        .unwrap();
    let requests = drain(&mut registered);
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].url(), "https://example.com");
    assert!(requests[0].matches_nonce(&hex_nonce(EXPECTED_NONCE)));
}

fn drain(terminal: &mut Terminal) -> Vec<zmux_terminal_adapter::ProgramOpenRequest> {
    match terminal.take_open_requests() {
        Ok(requests) => requests,
        Err(error) => panic!("{error}"),
    }
}

#[test]
fn malformed_explicit_open_fields_are_ignored() {
    let mut terminal = Terminal::new(Size { rows: 2, cols: 20 }).unwrap();
    for event in [
        format!("\x1b]777;zmux-open;v1;{NONCE};{URL_B64}=\x07"),
        format!("\x1b]777;zmux-open;v1;{NONCE};aHR0cHM6Ly9leGFtcGxlLmNvbf8\x07"),
        format!(
            "\x1b]777;zmux-open;v1;{};{URL_B64}\x07",
            NONCE.to_ascii_uppercase()
        ),
        format!("\x1b]777;zmux-open;v2;{NONCE};{URL_B64}\x07"),
    ] {
        terminal.process(event.as_bytes()).unwrap();
    }
    let oversized = encode_base64url(&vec![b'a'; 8_193]);
    terminal
        .process(format!("\x1b]777;zmux-open;v1;{NONCE};{oversized}\x07").as_bytes())
        .unwrap();
    assert!(terminal.take_open_requests().unwrap().is_empty());
}

fn hex_nonce(value: &str) -> [u8; 16] {
    let mut nonce = [0; 16];
    for (index, byte) in nonce.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16).unwrap();
    }
    nonce
}

fn encode_base64url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut encoded = String::new();
    for chunk in bytes.chunks(3) {
        let a = chunk[0];
        let b = chunk.get(1).copied();
        let c = chunk.get(2).copied();
        encoded.push(char::from(ALPHABET[(a >> 2) as usize]));
        encoded.push(char::from(
            ALPHABET[(((a & 0x03) << 4) | (b.unwrap_or(0) >> 4)) as usize],
        ));
        if let Some(b) = b {
            encoded.push(char::from(
                ALPHABET[(((b & 0x0f) << 2) | (c.unwrap_or(0) >> 6)) as usize],
            ));
        }
        if let Some(c) = c {
            encoded.push(char::from(ALPHABET[(c & 0x3f) as usize]));
        }
    }
    encoded
}

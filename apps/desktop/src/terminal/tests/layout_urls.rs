use super::super::*;

#[test]
fn pending_session_reports_initial_layout_once_without_resize_claim() {
    let (binding, rx) = TerminalSessionBinding::pending(TerminalPtyConfig::default());

    let initial = binding.record_layout(120, 36);
    assert!(initial.initialized);
    assert!(!initial.resized);

    assert_eq!(
        rx.recv_timeout(Duration::from_millis(10)).unwrap(),
        (120, 36)
    );
    let resize = binding.record_layout(121, 37);
    assert!(!resize.initialized);
    assert!(resize.resized);
    assert!(rx.try_recv().is_err());
}
#[test]
fn terminal_grid_dimension_tolerates_float_precision() {
    for cell in [8.1_f32, 9.7, 14.1, 20.1] {
        for count in 20..=200 {
            let available = count as f32 * cell;
            assert_eq!(terminal_grid_dimension(available, cell, 20), count);
        }
    }
    assert_eq!(terminal_grid_dimension(1.0, 8.0, 20), 20);
    assert_eq!(terminal_grid_dimension(1.0, 18.0, 1), 1);
}
#[test]
fn detects_plain_terminal_urls_at_cell() {
    let mut state = TerminalModel::new_for_test(80, 4, 100);
    state.process_bytes(b"open https://example.com/path?x=1.\r\n");
    state.handle.publish_snapshot();
    let snapshot = state.handle.snapshot();

    let link = terminal_link_at_cell(&snapshot, TerminalCellPoint { row: 0, col: 12 })
        .expect("url under cursor");

    assert_eq!(link.url, "https://example.com/path?x=1");
    assert_eq!(link.line, 0);
    assert_eq!(link.range, 5..33);
    assert!(terminal_link_at_cell(&snapshot, TerminalCellPoint { row: 0, col: 2 }).is_none());
}

#[test]
fn detects_plain_terminal_urls_across_soft_wrapped_rows() {
    let url = "https://example.com/a/very/long/path?x=1";
    let prefix = "open ";
    let columns = 12;
    let mut state = TerminalModel::new_for_test(columns, 6, 100);
    state.process_bytes(format!("{prefix}{url}").as_bytes());
    state.handle.publish_snapshot();
    let snapshot = state.handle.snapshot();

    for url_offset in [2, 18, url.len() - 2] {
        let click_offset = prefix.len() + url_offset;
        let link = terminal_link_at_cell(
            &snapshot,
            TerminalCellPoint {
                row: click_offset / columns,
                col: click_offset % columns,
            },
        )
        .expect("wrapped URL segment under pointer");
        assert_eq!(link.url, url);
    }
}
#[test]
fn plain_url_detection_uses_terminal_columns() {
    let row_text = vec![
        (0, '中'),
        (2, ' '),
        (3, 'h'),
        (4, 't'),
        (5, 't'),
        (6, 'p'),
        (7, 's'),
        (8, ':'),
        (9, '/'),
        (10, '/'),
        (11, 'e'),
        (12, 'x'),
        (13, '.'),
        (14, 'c'),
        (15, 'o'),
        (16, 'm'),
        (17, ')'),
    ];

    let (url, range) = terminal_plain_url_at(&row_text, 12).expect("url under cursor");

    assert_eq!(url, "https://ex.com");
    assert_eq!(range, 3..17);
}
#[test]
fn plain_url_detection_matches_xterm_style_boundaries() {
    let row_text: Vec<(usize, char)> = "(HTTPS://example.com/a?q=1),".chars().enumerate().collect();

    let (url, range) = terminal_plain_url_at(&row_text, 4).expect("url under cursor");

    assert_eq!(url, "HTTPS://example.com/a?q=1");
    assert_eq!(range, 1..26);
    assert!(terminal_plain_url_at(&row_text, 0).is_none());
    assert!(terminal_plain_url_at(&row_text, 26).is_none());
}
#[test]
fn plain_url_detection_supports_file_urls() {
    let row_text: Vec<(usize, char)> = "open file:///tmp/codux-log.txt."
        .chars()
        .enumerate()
        .collect();

    let (url, range) = terminal_plain_url_at(&row_text, 12).expect("file url under cursor");

    assert_eq!(url, "file:///tmp/codux-log.txt");
    assert_eq!(range, 5..30);
}

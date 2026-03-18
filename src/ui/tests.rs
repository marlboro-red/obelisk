use super::helpers::compute_pty_area;

/// 80x24 — the classic "small terminal" target.
/// Compact rows (<40) → chrome=7, no diagnostics panel (<148 cols).
#[test]
fn pty_area_80x24() {
    let (rows, cols) = compute_pty_area(80, 24);
    // content=17, output=14, inner=12×78
    assert_eq!(rows, 12);
    assert_eq!(cols, 78);
}

/// 120x40 — normal chrome, but still below the diagnostics breakpoint.
#[test]
fn pty_area_120x40() {
    let (rows, cols) = compute_pty_area(120, 40);
    // content=28, output=25, inner=23×118
    assert_eq!(rows, 23);
    assert_eq!(cols, 118);
}

/// 100x30 — compact rows, no diagnostics panel.
#[test]
fn pty_area_100x30() {
    let (rows, cols) = compute_pty_area(100, 30);
    // compact chrome=7, content=23, output=20, inner=18×98
    assert_eq!(rows, 18);
    assert_eq!(cols, 98);
}

/// 200x50 — large terminal: normal chrome, wider diagnostics present.
#[test]
fn pty_area_large_terminal() {
    let (rows, cols) = compute_pty_area(200, 50);
    // chrome=12, content=38, output=35, inner=33×(200-56-2)=142
    assert_eq!(rows, 33);
    assert_eq!(cols, 142);
}

/// Very small terminal — saturating_sub prevents underflow.
#[test]
fn pty_area_tiny_terminal() {
    let (rows, cols) = compute_pty_area(20, 10);
    // compact chrome=7, content=3, output=0, inner=0×18
    // (saturating_sub keeps it at 0, not negative)
    assert!(rows <= 1, "rows should be ≤1 at 10 rows high, got {rows}");
    assert!(cols > 0, "cols should be positive at 20 cols wide, got {cols}");
}

/// The wider diagnostics panel stays hidden until 148 cols.
#[test]
fn pty_area_sidebar_threshold() {
    let (_, cols_147) = compute_pty_area(147, 50);
    let (_, cols_148) = compute_pty_area(148, 50);
    // 147: no diagnostics → inner = 147-2 = 145
    // 148: diagnostics visible → inner = 148-56-2 = 90
    assert_eq!(cols_147, 145);
    assert_eq!(cols_148, 90);
}

/// At exactly 39 rows, compact mode; at 40, normal mode.
#[test]
fn pty_area_compact_row_threshold() {
    let (rows_39, _) = compute_pty_area(80, 39);
    let (rows_40, _) = compute_pty_area(80, 40);
    // 39: compact chrome=7, content=32, output=29, inner=27
    // 40: normal chrome=12, content=28, output=25, inner=23
    assert_eq!(rows_39, 27);
    assert_eq!(rows_40, 23);
    // Compact mode gives MORE content rows because chrome is smaller
    assert!(rows_39 > rows_40);
}

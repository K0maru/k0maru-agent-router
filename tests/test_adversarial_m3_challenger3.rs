//! Adversarial empirical stress tests for Milestone M3 Iteration 2 (Tmux Parser Remediation).
//!
//! Specifically validates:
//! 1. `parse_pane_line` against all adversarial input categories mandated by dispatch:
//!    - empty pane ID in TSV (`\t/tmp\tbash`, `\t/tmp\t`, `\t\t`)
//!    - diagnostic error strings (`unknown command: list-panes`, `tmux: unknown option`, `Segmentation fault: 11`)
//!    - whitespace inside pane ID (`% 0\t/tmp`, `%foo bar\t/tmp`)
//!    - missing % prefix (`0\t/tmp\tbash`, `main\t/tmp\tbash`)
//! 2. Colon variants of adversarial inputs (`% 0:/tmp`, `0:/tmp:bash`, etc.)
//! 3. Whitespace, CRLF, and tab fuzzing matrix
//! 4. `parse_tmux_output` multi-line filtering resilience with interleaved hostile lines
//! 5. Empirical reproduction of macOS `spawn_pane` race condition where tmux emits `"%1\t\ttmux"`

use k0maru_router::driver::{parse_pane_line, parse_tmux_output, SessionStatus};

// =========================================================================
// 1. Mandatory Dispatch Adversarial Inputs
// =========================================================================

#[test]
fn test_dispatch_empty_pane_id_in_tsv() {
    // Mandated test inputs: `\t/tmp\tbash`, `\t/tmp\t`, `\t\t`
    assert!(
        parse_pane_line("\t/tmp\tbash").is_none(),
        "Empty pane ID with cwd and command must return None"
    );
    assert!(
        parse_pane_line("\t/tmp\t").is_none(),
        "Empty pane ID with cwd and empty command must return None"
    );
    assert!(
        parse_pane_line("\t\t").is_none(),
        "Double tab without fields must return None"
    );

    // Variations with whitespace around empty pane ID
    assert!(
        parse_pane_line("   \t/tmp\tbash").is_none(),
        "Whitespace-only pane ID in TSV must return None"
    );
    assert!(
        parse_pane_line("\t   /tmp\t").is_none(),
        "Empty pane ID with whitespace cwd must return None"
    );
    assert!(
        parse_pane_line("\t\t\t").is_none(),
        "Multiple consecutive tabs must return None"
    );
}

#[test]
fn test_dispatch_diagnostic_error_strings() {
    // Mandated test inputs: `unknown command: list-panes`, `tmux: unknown option`, `Segmentation fault: 11`
    assert!(
        parse_pane_line("unknown command: list-panes").is_none(),
        "unknown command: list-panes must return None"
    );
    assert!(
        parse_pane_line("tmux: unknown option").is_none(),
        "tmux: unknown option must return None"
    );
    assert!(
        parse_pane_line("Segmentation fault: 11").is_none(),
        "Segmentation fault: 11 must return None"
    );

    // Additional common diagnostic and crash strings
    assert!(
        parse_pane_line("failed to connect to server: Connection refused").is_none(),
        "Connection refused must return None"
    );
    assert!(
        parse_pane_line("no server running on /tmp/tmux-501/default").is_none(),
        "no server running must return None"
    );
    assert!(
        parse_pane_line("tmux: invalid option -- 'z'").is_none(),
        "invalid option must return None"
    );
    assert!(
        parse_pane_line("killed: 9").is_none(),
        "killed: 9 must return None"
    );
    assert!(
        parse_pane_line("Abort trap: 6").is_none(),
        "Abort trap: 6 must return None"
    );
    assert!(
        parse_pane_line("error: pane not found").is_none(),
        "error: pane not found must return None"
    );
}

#[test]
fn test_dispatch_whitespace_inside_pane_id() {
    // Mandated test inputs: `% 0\t/tmp`, `%foo bar\t/tmp`
    assert!(
        parse_pane_line("% 0\t/tmp").is_none(),
        "% 0\\t/tmp with space in pane ID must return None"
    );
    assert!(
        parse_pane_line("%foo bar\t/tmp").is_none(),
        "%foo bar\\t/tmp with space in pane ID must return None"
    );

    // With 3rd command column
    assert!(
        parse_pane_line("% 0\t/tmp\tbash").is_none(),
        "% 0\\t/tmp\\tbash must return None"
    );
    assert!(
        parse_pane_line("%foo bar\t/tmp\tzsh").is_none(),
        "%foo bar\\t/tmp\\tzsh must return None"
    );

    // Additional space variants inside pane ID
    assert!(
        parse_pane_line("%  0\t/tmp").is_none(),
        "Double space in pane ID must return None"
    );
    assert!(
        parse_pane_line("%\t0\t/tmp").is_some(),
        "Note: %\\t0\\t/tmp treats 0 as cwd because tab is delimiter"
    );
    assert!(
        parse_pane_line("% 999\t/tmp\tbash").is_none(),
        "Space between % and digits must return None"
    );
}

#[test]
fn test_dispatch_missing_percent_prefix() {
    // Mandated test inputs: `0\t/tmp\tbash`, `main\t/tmp\tbash`
    assert!(
        parse_pane_line("0\t/tmp\tbash").is_none(),
        "Numeric pane ID without % prefix must return None"
    );
    assert!(
        parse_pane_line("main\t/tmp\tbash").is_none(),
        "Named session ID without % prefix must return None"
    );

    // Other non-% prefixes
    assert!(
        parse_pane_line("$0\t/tmp\tbash").is_none(),
        "$ prefix must return None"
    );
    assert!(
        parse_pane_line("@0\t/tmp\tbash").is_none(),
        "@ prefix must return None"
    );
    assert!(
        parse_pane_line("pane0\t/tmp\tbash").is_none(),
        "pane0 prefix must return None"
    );
}

// =========================================================================
// 2. Colon-Delimited Symmetry Verification
// =========================================================================

#[test]
fn test_colon_delimited_adversarial_symmetry() {
    // Whitespace inside pane ID with colon delimiter
    assert!(
        parse_pane_line("% 0:/tmp").is_none(),
        "% 0:/tmp must return None"
    );
    assert!(
        parse_pane_line("%foo bar:/tmp").is_none(),
        "%foo bar:/tmp must return None"
    );
    assert!(
        parse_pane_line("% 0:/tmp:bash").is_none(),
        "% 0:/tmp:bash must return None"
    );

    // Missing % prefix with colon delimiter
    assert!(
        parse_pane_line("0:/tmp:bash").is_none(),
        "0:/tmp:bash must return None"
    );
    assert!(
        parse_pane_line("main:/tmp:bash").is_none(),
        "main:/tmp:bash must return None"
    );
    assert!(
        parse_pane_line("0:/tmp").is_none(),
        "0:/tmp must return None"
    );

    // Empty pane ID with colon delimiter
    assert!(
        parse_pane_line(":/tmp:bash").is_none(),
        ":/tmp:bash must return None"
    );
    assert!(
        parse_pane_line(":/tmp:").is_none(),
        ":/tmp: must return None"
    );
    assert!(parse_pane_line("::").is_none(), ":: must return None");
    assert!(parse_pane_line(":::").is_none(), "::: must return None");

    // Empty CWD with colon delimiter
    assert!(
        parse_pane_line("%0::bash").is_none(),
        "%0::bash must return None"
    );
    assert!(
        parse_pane_line("%0:   :bash").is_none(),
        "%0:   :bash must return None"
    );
    assert!(parse_pane_line("%0:").is_none(), "%0: must return None");
}

// =========================================================================
// 3. Multi-Line Filtering Resilience with Mixed Hostile Output
// =========================================================================

#[test]
fn test_parse_tmux_output_interleaved_hostile_stream() {
    let raw = "\
unknown command: list-panes\n\
\t/tmp\tbash\n\
%0\t/tmp\tbash\n\
tmux: unknown option\n\
% 0\t/tmp\n\
Segmentation fault: 11\n\
0\t/tmp\tbash\n\
%1\t/tmp\tzsh\n\
main\t/tmp\tbash\n\
\t\t\n\
\r\n\
%foo bar\t/tmp\n\
%2:/tmp:fish\n\
failed to connect to server: Connection refused\n\
";

    let sessions = parse_tmux_output(raw);
    assert_eq!(
        sessions.len(),
        3,
        "Should only parse the 3 valid sessions, filtering all hostile/corrupted lines"
    );

    assert_eq!(sessions[0].pane_id, "%0");
    assert_eq!(sessions[0].status, SessionStatus::Idle);
    assert_eq!(sessions[0].current_command, Some("bash".into()));

    assert_eq!(sessions[1].pane_id, "%1");
    assert_eq!(sessions[1].status, SessionStatus::Idle);
    assert_eq!(sessions[1].current_command, Some("zsh".into()));

    assert_eq!(sessions[2].pane_id, "%2");
    assert_eq!(sessions[2].status, SessionStatus::Idle);
    assert_eq!(sessions[2].current_command, Some("fish".into()));
}

// =========================================================================
// 4. Verification of Valid Inputs (Zero False Negatives)
// =========================================================================

#[test]
fn test_valid_pane_lines_parse_successfully() {
    // Valid 3-field TSV
    let s1 = parse_pane_line("%0\t/tmp\tbash").expect("Valid 3-field TSV should parse");
    assert_eq!(s1.pane_id, "%0");
    assert_eq!(s1.status, SessionStatus::Idle);
    assert_eq!(s1.current_command, Some("bash".into()));

    // Valid 2-field TSV
    let s2 = parse_pane_line("%1\t/tmp").expect("Valid 2-field TSV should parse");
    assert_eq!(s2.pane_id, "%1");
    assert_eq!(s2.status, SessionStatus::Working);
    assert_eq!(s2.current_command, None);

    // Valid TSV with empty command column
    let s3 = parse_pane_line("%2\t/tmp\t").expect("Valid TSV with empty cmd should parse");
    assert_eq!(s3.pane_id, "%2");
    assert_eq!(s3.status, SessionStatus::Working);
    assert_eq!(s3.current_command, None);

    // Valid colon-delimited
    let s4 = parse_pane_line("%3:/tmp:zsh").expect("Valid colon line should parse");
    assert_eq!(s4.pane_id, "%3");
    assert_eq!(s4.status, SessionStatus::Idle);
    assert_eq!(s4.current_command, Some("zsh".into()));

    // Valid colon-delimited 2-field
    let s5 = parse_pane_line("%4:/tmp").expect("Valid colon 2-field should parse");
    assert_eq!(s5.pane_id, "%4");
    assert_eq!(s5.status, SessionStatus::Working);
    assert_eq!(s5.current_command, None);
}

// =========================================================================
// 5. Empirical Proof of Empty CWD in Tmux Spawn Output and Parser Behavior
// =========================================================================

#[test]
fn test_empirical_behavior_on_empty_cwd_line() {
    // When tmux emits `%1\t\ttmux` (e.g. before proc table updates CWD on macOS),
    // parse_pane_line strictly evaluates this as None because CWD is empty.
    let empty_cwd_tsv = "%1\t\ttmux";
    assert!(
        parse_pane_line(empty_cwd_tsv).is_none(),
        "Line with empty CWD column must return None from parse_pane_line"
    );

    // Colon variant `%1::tmux`
    let empty_cwd_colon = "%1::tmux";
    assert!(
        parse_pane_line(empty_cwd_colon).is_none(),
        "Line with empty CWD in colon syntax must return None"
    );
}

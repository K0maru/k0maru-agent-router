# TEST_READY.md — Test Suite Readiness Declaration

**Project**: `k0maru-agent-router`  
**Status**: `TEST_READY` (100% Pass across Tiers 1–5)  
**Date**: `2026-10-11T23:13:00Z`  
**Toolchain**: Rust 2021 Edition (MSRV 1.75+)  
**Zero-Warning Quality Gate**: Formatter clean (`cargo fmt`), strict clippy clean (`cargo clippy --all-targets -- -D warnings`), 100% passing tests (`cargo test`).

---

## 1. Test Runner Commands

### Standard Verification Commands
```bash
# Full test suite execution (All 347 tests across unit, integration, and adversarial suites)
cargo test

# Execute with uncaptured stdout/stderr
cargo test -- --nocapture

# Execute by test binary target
cargo test --test test_envelope
cargo test --test test_m1_contract_integrity
cargo test --test test_adversarial_m1
cargo test --test test_boundary_outbox
cargo test --test test_adversarial_m2
cargo test --test test_adversarial_m2_lifecycle
cargo test --test test_adversarial_m2_stress
cargo test --test test_addressing_tmux
cargo test --test test_adversarial_m3
cargo test --test test_adversarial_m3_challenger2
cargo test --test test_adversarial_m3_challenger3
cargo test --test test_cli_coordinator
cargo test --test test_adversarial_m4
cargo test --test test_adversarial_m4_challenger2
cargo test --test test_adversarial_tier5_part1
cargo test --test test_adversarial_tier5_part2
cargo test --test test_adversarial_tier5_challenger3

# Execute by tier naming filter
cargo test tier1_
cargo test tier2_
cargo test tier3_
cargo test tier4_
cargo test tier5_
```

### Full Quality Verification Gate (Zero-Warning Standard)
```bash
# 1. Format verification (zero diffs)
cargo fmt --check

# 2. Strict linter gate (zero warnings across all targets)
cargo clippy --all-targets -- -D warnings

# 3. Full test run
cargo test
```

---

## 2. Test Coverage Summary Table

| Tier | Category / Focus | Test Count | Pass Rate | Status |
|:---|:---|:---:|:---:|:---:|
| **Tier 1** | **Feature Coverage**: Nominal happy-path & public protocol contracts | 44 | 100% (44/44) | ✅ PASS |
| **Tier 2** | **Boundary & Corner Cases**: Stress, malformed payloads, corruption, limits & error paths | 130 | 100% (130/130) | ✅ PASS |
| **Tier 3** | **Cross-Feature Combinations**: Multi-module pipeline dataflow & state machine transitions | 25 | 100% (25/25) | ✅ PASS |
| **Tier 4** | **Real-World Scenarios**: End-to-end user workflows, hermetic tmux lifecycle, and CLI integration | 16 | 100% (16/16) | ✅ PASS |
| **Tier 5** | **Adversarial Coverage Hardening** (Part 1: 21, Part 2: 21, Challenger 3: 13) | 55 | 100% (55/55) | ✅ PASS |
| **Subtotal (Integration / E2E)** | **Integration & Adversarial Suites (`tests/`)** | **270** | **100% (270/270)** | **✅ PASS** |
| **Unit Tests** | **Internal Module Logic (`src/lib.rs`)** | **77** | **100% (77/77)** | **✅ PASS** |
| **Total Test Suite** | **Complete Workspace Verification** | **347** | **100% (347/347)** | **✅ PASS** |

---

## 3. Integration & Adversarial Test Suites Breakdown (`tests/`)

| Test File | Milestone / Focus | Tests | Status | Key Coverage Areas |
|:---|:---:|:---:|:---:|:---|
| `tests/test_envelope.rs` | M1 / Protocol | 13 | ✅ Pass | Full JSON Serde, minimal omissions, RFC-3339 timestamps, large payload stress (128KB), malformed syntax |
| `tests/test_m1_contract_integrity.rs` | M1 / Integrity | 18 | ✅ Pass | Serialization idempotency, unknown fields tolerance, severity case-insensitivity, exact error taxonomy |
| `tests/test_adversarial_m1.rs` | M1 / Adversarial | 11 | ✅ Pass | Path traversal in IDs (`../evil`), null bytes, 1MB string stress, deep nested JSON, Unicode bidi & ANSI |
| `tests/test_boundary_outbox.rs` | M2 / Policy & Outbox | 16 | ✅ Pass | Boundary verdicts, MRE markdown draft generator, directory auto-creation, special char escaping, batch isolation |
| `tests/test_adversarial_m2.rs` | M2 / Adversarial | 9 | ✅ Pass | Spoofed workspace resistance, hostile Unicode/RTL identifiers, outbox path traversal attacks, dirty dirs |
| `tests/test_adversarial_m2_lifecycle.rs` | M2 / State Machine | 20 | ✅ Pass | Full lifecycle (Pending -> Approved / Rejected), illegal re-transitions, re-staging resets, sorting determinism |
| `tests/test_adversarial_m2_stress.rs` | M2 / Stress | 14 | ✅ Pass | 5MB reproduction payloads, corrupted status markers, CRLF line ending matrix, metadata spoofing isolation |
| `tests/test_addressing_tmux.rs` | M3 / Physical Addressing | 27 | ✅ Pass | Path canonicalization, symlink resolution, session matching (Idle vs Working), hermetic tmux socket lifecycle |
| `tests/test_adversarial_m3.rs` | M3 / Adversarial | 18 | ✅ Pass | Cyclic symlinks, device nodes (`/dev/null`), macOS `/tmp` vs `/private/tmp`, spaces in paths, shell classifier |
| `tests/test_adversarial_m3_challenger2.rs` | M3 / Parser & Robustness | 17 | ✅ Pass | Colon-in-path parser stress, offline server fallback, live keystroke injection defense, multi-session priority |
| `tests/test_adversarial_m3_challenger3.rs` | M3 / Stream Parser | 8 | ✅ Pass | Hostile interleaved output streams, whitespace in pane IDs, empty CWD lines, TSV/colon delimiter symmetry |
| `tests/test_cli_coordinator.rs` | M4 / CLI Integration | 24 | ✅ Pass | Subcommands `status`, `route`, `probe`, `outbox [list/inspect/approve/reject]`, dry-run mode, JSON formatting |
| `tests/test_adversarial_m4.rs` | M4 / CLI Robustness | 8 | ✅ Pass | Exit codes (1 for runtime/domain error, 2 for CLI arg parse error), dry-run filesystem immutability |
| `tests/test_adversarial_m4_challenger2.rs` | M4 / CLI E2E | 12 | ✅ Pass | Top-level convenience aliases (`list-staged`, `approve`), quarantine invariant, draft tamper safety |
| `tests/test_adversarial_tier5_part1.rs` | M5 / Adversarial White-Box Hardening | 21 | ✅ Pass | Error taxonomy matrix, reproduction value handling, exotic RFC-3339 timestamps, outbox anti-tampering & concurrent restaging |
| `tests/test_adversarial_tier5_part2.rs` | M5 / Adversarial White-Box Hardening | 21 | ✅ Pass | macOS tmp path identity, redundant slashes/dots, delimiter fuzzing, dry-run invariants, keystroke injection defense, CLI JSON contracts |
| `tests/test_adversarial_tier5_challenger3.rs` | M5 / Adversarial Empirical Stress | 13 | ✅ Pass | Error source chaining, nanosecond/leap timestamps, multi-threaded approval race condition, status header/trailer anti-spoofing |

---

## 4. Feature Checklist Mapping (All 27 Features from `PROJECT.md`)

All 27 features from `PROJECT.md § Feature Inventory` are mapped across Tiers 1–4 with complete, self-contained test verification.

### Milestone 1: R1 Task Envelope Protocol & Contract Validation
- [x] **Feature 1: `agent-task-v1` JSON Schema & Structs**
  - **Tier 1**: `tier1_envelope_full_json_deserialization`, `tier1_envelope_minimal_optional_fields_omitted`, `tier1_envelope_roundtrip_serde_preservation`, `test_roundtrip_canonical_envelope`, `test_roundtrip_minimal_envelope_with_optional_fields_omitted`, `test_roundtrip_external_target_with_and_without_workspace`
  - **Tier 2**: `tier2_envelope_empty_json_string_fails`, `tier2_envelope_malformed_syntax_fails`, `tier2_envelope_missing_required_fields_fails`, `tier2_envelope_invalid_field_types_fails`, `test_unknown_fields_in_top_level_and_sub_structs`, `test_unknown_field_typos_in_optional_keys`, `test_optional_fields_omitted_vs_explicit_null`
  - **Tier 3**: `tier3_multi_envelope_batch_staging_isolation`, `test_contrast_internal_task_invokes_driver_unlike_external_quarantine`
  - **Tier 4**: `tier4_scenario_upstream_oss_quarantine_workflow`, `tier4_scenario_internal_service_direct_routing`

- [x] **Feature 2: Strict Envelope Validation**
  - **Tier 1**: `tier1_envelope_target_external_flag_distinction`, `test_all_valid_severities_accepted_case_insensitively`
  - **Tier 2**: `test_empty_string_optional_fields_rejected_by_validation`, `test_internal_target_explicit_null_workspace_rejected`, `test_empty_and_whitespace_severity_rejection`, `test_invalid_severities_rejected_with_exact_contract_error`, `test_adversarial_id_path_traversal_rejections`, `test_adversarial_id_null_bytes_and_control_chars`, `test_adversarial_whitespace_only_rejections_across_all_fields`, `test_adversarial_target_workspace_invariants`, `test_exit_code_1_on_invalid_envelope_schema`
  - **Tier 3**: `test_cli_route_internal_missing_workspace_fails_fast`, `test_cli_route_untrusted_stages_to_outbox`
  - **Tier 4**: `test_quarantine_invariant_with_arbitrary_workspaces_never_checks_local_disk`

- [x] **Feature 3: RFC-3339 Timestamp Validation**
  - **Tier 1**: `tier1_envelope_rfc3339_timestamp_validation`
  - **Tier 2**: `tier2_envelope_invalid_timestamp_detection`, `test_timestamp_parse_error_is_typed_invalid_envelope`, `test_adversarial_timestamp_calendar_and_timezone_boundaries`, `test_adversarial_timestamp_leap_second_behavior`
  - **Tier 3**: `test_deterministic_sorting_by_timestamp_descending`, `test_deterministic_sorting_mixed_timestamps_and_tie_breakers`
  - **Tier 4**: `test_deterministic_sorting_subsecond_timestamps`, `test_deterministic_sorting_tie_breaker_by_id_ascending`

- [x] **Feature 4: Structured Reproduction Payload**
  - **Tier 1**: `tier1_envelope_structured_reproduction_payload`, `test_roundtrip_complex_structured_and_arbitrary_reproduction`
  - **Tier 2**: `tier2_envelope_adversarial_escaping_and_unicode`, `tier2_envelope_large_payload_handling`, `test_reproduction_empty_fields_in_object`, `test_adversarial_deeply_nested_json_in_reproduction`, `test_adversarial_massive_payloads_1mb_string`, `test_stress_massive_reproduction_payloads_5mb`, `test_stress_deeply_nested_json_reproduction`
  - **Tier 3**: `tier3_policy_evaluation_guides_staging`, `tier1_outbox_stage_markdown_structure`
  - **Tier 4**: `tier4_scenario_upstream_oss_quarantine_workflow`, `test_quarantine_cli_e2e_creates_structured_mre_draft`

- [x] **Feature 5: Typed Error Hierarchy**
  - **Tier 1**: `test_router_error_display_formatting`, `test_router_error_partial_eq`
  - **Tier 2**: `test_error_type_distinction_syntax_vs_validation_vs_io`, `test_non_string_severity_json_type_failure`, `test_adversarial_from_file_edge_cases`, `test_json_error_reporting_on_domain_errors`, `test_exit_code_1_on_nonexistent_files`, `test_exit_code_1_on_invalid_json`
  - **Tier 3**: `test_cli_error_corrupt_json`, `test_cli_error_missing_envelope_file`, `test_cli_error_empty_envelope_file`, `test_cli_error_unsupported_driver`
  - **Tier 4**: `test_cli_invalid_arguments_exit_code_2`, `test_exit_code_2_on_missing_required_arguments`

### Milestone 2: R2 Boundary Policy Engine & HITL Outbox Gate
- [x] **Feature 6: Boundary Policy Evaluation**
  - **Tier 1**: `tier1_boundary_policy_internal_target_verdict`, `tier1_boundary_policy_external_target_verdict`, `tier1_boundary_policy_untrusted_reason_content`, `test_adversarial_policy_verdict_predicates_and_serde`
  - **Tier 2**: `test_adversarial_policy_external_with_workspace_still_untrusted`, `test_adversarial_policy_target_unicode_and_hostile_identifiers`, `test_stress_boundary_policy_exotic_targets`
  - **Tier 3**: `tier3_policy_evaluation_guides_staging`, `test_cli_route_dry_run_untrusted_zero_filesystem_mutation`, `test_cli_route_internal_dry_run`
  - **Tier 4**: `tier4_scenario_internal_service_direct_routing`, `test_contrast_internal_task_invokes_driver_unlike_external_quarantine`

- [x] **Feature 7: Untrusted External Quarantine Trigger**
  - **Tier 1**: `tier1_boundary_policy_external_target_verdict`, `tier1_outbox_stage_creates_markdown_file`
  - **Tier 2**: `test_quarantine_invariant_driver_is_never_invoked`, `test_quarantine_invariant_with_arbitrary_workspaces_never_checks_local_disk`
  - **Tier 3**: `tier3_policy_evaluation_guides_staging`, `test_cli_route_untrusted_stages_to_outbox`, `test_cli_route_untrusted_json_output`
  - **Tier 4**: `tier4_scenario_upstream_oss_quarantine_workflow`, `test_quarantine_cli_e2e_creates_structured_mre_draft`

- [x] **Feature 8: MRE Markdown Draft Generator**
  - **Tier 1**: `tier1_outbox_stage_creates_markdown_file`, `tier1_outbox_stage_markdown_structure`, `tier1_outbox_stage_auto_creates_outbox_dir`
  - **Tier 2**: `tier2_outbox_stage_with_special_characters_in_payload`, `tier2_outbox_stage_none_optional_fields`, `tier2_outbox_stage_idempotent_overwrite`, `tier2_outbox_stage_distinct_ids_create_distinct_files`, `tier2_outbox_stage_unicode_and_emojis`, `test_stress_crlf_and_mixed_newlines_adversarial_matrix`, `test_stress_tricky_section_headers_and_boundary_isolation`, `test_stress_unicode_rtl_and_ansi_escapes`
  - **Tier 3**: `tier3_multi_envelope_batch_staging_isolation`
  - **Tier 4**: `tier4_scenario_upstream_oss_quarantine_workflow`

- [x] **Feature 9: Outbox Listing Engine**
  - **Tier 1**: `test_adversarial_query_normalization_variations`, `test_cli_outbox_list_empty_and_populated`
  - **Tier 2**: `test_adversarial_list_staged_dirty_directory_and_sorting`, `test_list_staged_resilience_to_directory_with_subdirectories_and_zero_byte_files`, `test_outbox_listing_tolerates_corrupted_and_foreign_files`, `test_stress_corrupted_junk_and_symlinks_tolerance`, `test_stress_summary_metadata_hijacking_via_expected_section`, `test_stress_nonexistent_and_empty_outbox_directory`
  - **Tier 3**: `test_deterministic_sorting_by_timestamp_descending`, `test_deterministic_sorting_mixed_timestamps_and_tie_breakers`
  - **Tier 4**: `test_deterministic_sorting_subsecond_timestamps`, `test_deterministic_sorting_tie_breaker_by_id_ascending`

- [x] **Feature 10: Outbox Inspection Engine**
  - **Tier 1**: `test_cli_outbox_inspect`
  - **Tier 2**: `test_adversarial_outbox_inspect_path_traversal_attempts`, `test_stress_inspect_approve_reject_path_traversal_rejections`, `test_adversarial_path_traversal_attempts_in_outbox_cli`, `test_cli_outbox_inspect_nonexistent_issue`, `test_outbox_operations_on_nonexistent_ids_fail_with_exit_1`
  - **Tier 3**: `test_cli_aliases_list_inspect_approve_reject`
  - **Tier 4**: `test_outbox_cli_full_lifecycle_and_duplicate_approve_rejection`

- [x] **Feature 11: Outbox Human Approval Engine**
  - **Tier 1**: `test_adversarial_outbox_lifecycle_state_machine`, `test_lifecycle_pending_to_approved_full_flow`
  - **Tier 2**: `test_lifecycle_duplicate_approval_rejected`, `test_lifecycle_approve_rejected_issue_rejected`, `test_tampered_draft_with_conflicting_status_rejected_safely`, `test_stress_approve_reject_corrupted_draft_missing_status`, `test_stress_header_trailer_mismatch_and_tampering_matrix`, `test_stress_status_spoofing_via_payload_injection`, `test_adversarial_content_injection_does_not_spoof_status`
  - **Tier 3**: `test_lifecycle_restaging_approved_resets_to_pending`, `test_lifecycle_restaging_multi_cycle_state_machine`, `test_cli_outbox_approve_lifecycle`
  - **Tier 4**: `test_concurrent_independent_draft_lifecycle_transitions`, `test_outbox_cli_full_lifecycle_and_duplicate_approve_rejection`

- [x] **Feature 12: Outbox Human Rejection Engine**
  - **Tier 1**: `test_adversarial_outbox_lifecycle_state_machine`, `test_lifecycle_pending_to_rejected_full_flow`
  - **Tier 2**: `test_lifecycle_duplicate_rejection_rejected`, `test_lifecycle_reject_approved_issue_rejected`, `test_corrupted_or_missing_status_markers_in_draft_file`, `test_lifecycle_operations_on_nonexistent_ids`, `test_lifecycle_operations_reject_path_traversal`, `test_stress_inspect_approve_reject_path_traversal_rejections`
  - **Tier 3**: `test_lifecycle_restaging_rejected_resets_to_pending`, `test_lifecycle_status_transitions_preserve_body_content`, `test_cli_outbox_reject_lifecycle`
  - **Tier 4**: `test_outbox_cli_full_lifecycle_and_duplicate_reject_rejection`

### Milestone 3: R3 Physical Addressing & Multiplexer Driver SPI
- [x] **Feature 13: Filesystem Path Canonicalization**
  - **Tier 1**: `test_normalize_existing_directory`, `test_normalize_relative_segments`, `test_normalize_symlinks`, `test_normalize_trailing_slashes`
  - **Tier 2**: `test_normalize_nonexistent_directory_fails_fast`, `test_normalize_broken_symlink_fails_fast`, `test_normalize_empty_or_whitespace_fails_fast`, `test_normalize_file_instead_of_directory_fails_fast`, `test_adversarial_cyclic_and_broken_symlinks`, `test_adversarial_device_nodes_rejected`, `test_adversarial_empty_and_whitespace_inputs`, `test_adversarial_nested_and_relative_symlink_chains`, `test_adversarial_nonexistent_directory_variations`, `test_adversarial_regular_files_rejected`, `test_adversarial_root_trailing_slashes`, `test_adversarial_spaces_in_directory_names`, `test_adversarial_symlinks_macos_tmp_vs_private_tmp`, `test_adversarial_trailing_slashes_combinations`, `test_adversarial_unicode_and_exotic_directory_names`, `test_preflight_validation_empty_and_whitespace_paths`, `test_preflight_validation_nonexistent_workspace`, `test_preflight_validation_file_as_workspace`, `test_preflight_validation_broken_symlink_as_workspace`
  - **Tier 3**: `test_resolve_with_symlink_workspace_canonical_match`, `test_adversarial_session_cwd_with_trailing_slashes_and_symlinks`
  - **Tier 4**: `test_adversarial_hermetic_tmux_spaces_symlinks_and_prompt_safety`

- [x] **Feature 14: Session State Matching & Resolution**
  - **Tier 1**: `test_resolve_hot_session_idle`, `test_resolve_hot_session_working`, `test_resolve_empty_sessions_cold_spawn`, `test_resolve_non_matching_sessions_cold_spawn`, `test_session_resolution_predicates_and_accessors`
  - **Tier 2**: `test_resolve_nonexistent_target_fails_fast`, `test_adversarial_session_resolution_stale_deleted_directories`, `test_resolve_session_stale_deleted_panes_ignored`
  - **Tier 3**: `test_resolve_priority_prefers_idle_over_working`, `test_adversarial_session_resolution_priority_matrix`, `test_resolve_session_multi_session_idle_priority`
  - **Tier 4**: `test_adversarial_session_resolution_serde_roundtrip`, `test_cli_probe_valid_directory`

- [x] **Feature 15: Hot Session Status Resolution**
  - **Tier 1**: `test_session_status_classification_matrix`
  - **Tier 2**: `test_adversarial_shell_classifier_exhaustive`
  - **Tier 3**: `test_resolve_priority_prefers_idle_over_working`, `test_adversarial_session_resolution_priority_matrix`
  - **Tier 4**: `test_resolve_hot_session_idle`, `test_resolve_hot_session_working`

- [x] **Feature 16: `MultiplexerDriver` Trait Contract**
  - **Tier 1**: `test_tmux_driver_is_available`, `test_session_resolution_predicates_and_accessors`
  - **Tier 2**: `test_tmux_driver_send_prompt_session_not_found`, `test_tmux_driver_spawn_pane_workspace_not_found_fails_fast`, `test_offline_server_send_prompt_fails_with_session_not_found`
  - **Tier 3**: `test_contrast_internal_task_invokes_driver_unlike_external_quarantine`, `test_quarantine_invariant_driver_is_never_invoked`
  - **Tier 4**: `test_tmux_driver_hermetic_lifecycle_with_isolated_socket`

- [x] **Feature 17: `TmuxDriver` Implementation**
  - **Tier 1**: `test_tmux_driver_is_available`, `test_tmux_driver_hermetic_lifecycle_with_isolated_socket`
  - **Tier 2**: `test_empirical_proof_raw_tmux_fallback_vs_k0maru_guard`, `test_keystroke_injection_defense_live_tmux`
  - **Tier 3**: `test_adversarial_hermetic_tmux_spaces_symlinks_and_prompt_safety`
  - **Tier 4**: `test_cli_probe_valid_directory`, `test_cli_probe_valid_directory_json`

- [x] **Feature 18: Tmux Output Parser**
  - **Tier 1**: `test_parse_tmux_output_tsv`, `test_parse_tmux_output_colon_delimited`, `test_parse_tmux_output_multiline_mixed`, `test_valid_pane_lines_parse_successfully`
  - **Tier 2**: `test_parse_tmux_output_empty_and_garbage_tolerance`, `test_parse_tmux_output_path_containing_colons`, `test_adversarial_tmux_parser_tsv_and_colon_varieties`, `test_reproduce_defect_1_leading_tab_erasure_misattributes_fields`, `test_reproduce_defect_2_unvalidated_colon_fallback_parses_arbitrary_errors`, `test_reproduce_defect_3_two_field_colon_path_with_colon_truncated`, `test_stress_parser_missing_fields_and_truncated_lines`, `test_stress_parser_paths_with_multiple_colons_tsv`, `test_stress_parser_paths_with_multiple_colons_colon_delimited`, `test_stress_parser_whitespace_crlf_and_giant_lines`, `test_dispatch_empty_pane_id_in_tsv`, `test_dispatch_missing_percent_prefix`, `test_dispatch_whitespace_inside_pane_id`, `test_empirical_behavior_on_empty_cwd_line`, `test_parse_tmux_output_interleaved_hostile_stream`, `test_colon_delimited_adversarial_symmetry`
  - **Tier 3**: `test_dispatch_diagnostic_error_strings`
  - **Tier 4**: `test_adversarial_hermetic_tmux_spaces_symlinks_and_prompt_safety`

- [x] **Feature 19: Tmux Server Graceful Detection**
  - **Tier 1**: `test_tmux_driver_offline_graceful_empty_list`
  - **Tier 2**: `test_offline_server_returns_empty_vec_not_err`, `test_offline_server_send_prompt_fails_with_session_not_found`
  - **Tier 3**: `test_cli_probe_valid_directory`
  - **Tier 4**: `test_cli_status_text`, `test_cli_status_json`

- [x] **Feature 20: Tmux Prompt Submission**
  - **Tier 1**: `test_tmux_driver_hermetic_lifecycle_with_isolated_socket`
  - **Tier 2**: `test_tmux_driver_send_prompt_session_not_found`, `test_keystroke_injection_defense_live_tmux`
  - **Tier 3**: `test_adversarial_hermetic_tmux_spaces_symlinks_and_prompt_safety`
  - **Tier 4**: `test_empirical_proof_raw_tmux_fallback_vs_k0maru_guard`

- [x] **Feature 21: Tmux Pane Spawning**
  - **Tier 1**: `test_tmux_driver_hermetic_lifecycle_with_isolated_socket`
  - **Tier 2**: `test_tmux_driver_spawn_pane_workspace_not_found_fails_fast`
  - **Tier 3**: `test_adversarial_hermetic_tmux_spaces_symlinks_and_prompt_safety`
  - **Tier 4**: `test_cli_probe_valid_directory`

### Milestone 4: R4 Router Coordinator Engine, CLI & Integration
- [x] **Feature 22: Router Coordinator Pipeline**
  - **Tier 1**: `test_cli_route_untrusted_stages_to_outbox`, `test_cli_route_internal_dry_run`
  - **Tier 2**: `test_cli_route_dry_run_untrusted_zero_filesystem_mutation`, `test_dry_run_leaves_filesystem_and_outbox_completely_untouched`, `test_contrast_internal_task_invokes_driver_unlike_external_quarantine`, `test_quarantine_invariant_driver_is_never_invoked`
  - **Tier 3**: `test_cli_route_internal_missing_workspace_fails_fast`, `test_cli_route_untrusted_json_output`, `test_cli_route_internal_dry_run_json`
  - **Tier 4**: `test_quarantine_cli_e2e_creates_structured_mre_draft`

- [x] **Feature 23: `k0maru-router status` Subcommand**
  - **Tier 1**: `tier1_cli_status_runner_helper`, `test_cli_status_text`, `test_cli_status_json`
  - **Tier 2**: `test_json_formatting_across_all_subcommands`
  - **Tier 3**: `test_cli_status_json`
  - **Tier 4**: `test_json_formatting_across_all_subcommands`

- [x] **Feature 24: `k0maru-router route --file <path>`**
  - **Tier 1**: `test_cli_route_untrusted_stages_to_outbox`, `test_cli_route_internal_dry_run`
  - **Tier 2**: `test_cli_error_missing_envelope_file`, `test_cli_error_empty_envelope_file`, `test_cli_error_corrupt_json`, `test_exit_code_1_on_nonexistent_files`, `test_exit_code_1_on_invalid_json`, `test_exit_code_1_on_invalid_envelope_schema`, `test_exit_code_1_on_invalid_workspace`
  - **Tier 3**: `test_cli_route_untrusted_json_output`, `test_cli_route_internal_dry_run_json`, `test_cli_route_internal_missing_workspace_fails_fast`
  - **Tier 4**: `test_quarantine_cli_e2e_creates_structured_mre_draft`

- [x] **Feature 25: `k0maru-router probe --workspace <path>`**
  - **Tier 1**: `test_cli_probe_valid_directory`, `test_cli_probe_valid_directory_json`
  - **Tier 2**: `test_cli_probe_missing_workspace_arg`, `test_cli_probe_nonexistent_directory`, `test_cli_probe_nonexistent_directory_json`, `test_exit_code_1_on_invalid_workspace`
  - **Tier 3**: `test_cli_probe_valid_directory_json`
  - **Tier 4**: `test_json_formatting_across_all_subcommands`

- [x] **Feature 26: `k0maru-router outbox` Subcommands**
  - **Tier 1**: `test_cli_outbox_list_empty_and_populated`, `test_cli_outbox_inspect`, `test_cli_outbox_approve_lifecycle`, `test_cli_outbox_reject_lifecycle`
  - **Tier 2**: `test_cli_outbox_inspect_nonexistent_issue`, `test_outbox_operations_on_nonexistent_ids_fail_with_exit_1`, `test_adversarial_path_traversal_attempts_in_outbox_cli`, `test_outbox_listing_tolerates_corrupted_and_foreign_files`, `test_tampered_draft_with_conflicting_status_rejected_safely`
  - **Tier 3**: `test_outbox_cli_full_lifecycle_and_duplicate_approve_rejection`, `test_outbox_cli_full_lifecycle_and_duplicate_reject_rejection`
  - **Tier 4**: `test_outbox_cli_full_lifecycle_and_duplicate_approve_rejection`, `test_outbox_cli_full_lifecycle_and_duplicate_reject_rejection`

- [x] **Feature 27: Ergonomic CLI Aliases**
  - **Tier 1**: `test_cli_aliases_list_inspect_approve_reject`, `test_top_level_convenience_aliases_positional_and_json`
  - **Tier 2**: `test_top_level_aliases_missing_arguments_exit_code_2`
  - **Tier 3**: `test_top_level_convenience_aliases_positional_and_json`
  - **Tier 4**: `test_top_level_convenience_aliases_positional_and_json`

---

## 5. Verification Execution Log Summary

### A. Formatter Check (`cargo fmt --check`)
```
$ cargo fmt --check
Exit code: 0 (Clean, 0 formatting diffs)
```

### B. Strict Linter Check (`cargo clippy --all-targets -- -D warnings`)
```
$ cargo clippy --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.07s
Exit code: 0 (Zero warnings across library, binary, and test targets)
```

### C. Test Suite Execution (`cargo test`)
```
$ cargo test
     Running unittests src/lib.rs: 77 passed; 0 failed; 0 ignored; 0 measured
     Running unittests src/main.rs: 0 passed; 0 failed; 0 ignored; 0 measured
     Running tests/test_addressing_tmux.rs: 27 passed; 0 failed; 0 ignored; 0 measured
     Running tests/test_adversarial_m1.rs: 11 passed; 0 failed; 0 ignored; 0 measured
     Running tests/test_adversarial_m2.rs: 9 passed; 0 failed; 0 ignored; 0 measured
     Running tests/test_adversarial_m2_lifecycle.rs: 20 passed; 0 failed; 0 ignored; 0 measured
     Running tests/test_adversarial_m2_stress.rs: 14 passed; 0 failed; 0 ignored; 0 measured
     Running tests/test_adversarial_m3.rs: 18 passed; 0 failed; 0 ignored; 0 measured
     Running tests/test_adversarial_m3_challenger2.rs: 17 passed; 0 failed; 0 ignored; 0 measured
     Running tests/test_adversarial_m3_challenger3.rs: 8 passed; 0 failed; 0 ignored; 0 measured
     Running tests/test_adversarial_m4.rs: 8 passed; 0 failed; 0 ignored; 0 measured
     Running tests/test_adversarial_m4_challenger2.rs: 12 passed; 0 failed; 0 ignored; 0 measured
     Running tests/test_adversarial_tier5_challenger3.rs: 13 passed; 0 failed; 0 ignored; 0 measured
     Running tests/test_adversarial_tier5_part1.rs: 21 passed; 0 failed; 0 ignored; 0 measured
     Running tests/test_adversarial_tier5_part2.rs: 21 passed; 0 failed; 0 ignored; 0 measured
     Running tests/test_boundary_outbox.rs: 16 passed; 0 failed; 0 ignored; 0 measured
     Running tests/test_cli_coordinator.rs: 24 passed; 0 failed; 0 ignored; 0 measured
     Running tests/test_envelope.rs: 13 passed; 0 failed; 0 ignored; 0 measured
     Running tests/test_m1_contract_integrity.rs: 18 passed; 0 failed; 0 ignored; 0 measured

TOTAL: 347 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
Exit code: 0
```

---

## 6. Traceability Matrix to System Specifications

| Document & Section | Requirement / Contract | Coverage Highlights |
|:---|:---|:---|
| `ORIGINAL_REQUEST.md § R1` | `agent-task-v1` schema, Serde, RFC-3339, structured payload | Tested across `test_envelope.rs`, `test_m1_contract_integrity.rs`, `test_adversarial_m1.rs` |
| `ORIGINAL_REQUEST.md § R2` | `BoundaryPolicy` engine, `HitlOutbox` quarantine, MRE markdown drafts | Tested across `test_boundary_outbox.rs`, `test_adversarial_m2.rs`, `test_adversarial_m2_lifecycle.rs`, `test_adversarial_m2_stress.rs` |
| `ORIGINAL_REQUEST.md § R3` | Deterministic physical addressing, CWD path matching, `MultiplexerDriver` SPI | Tested across `test_addressing_tmux.rs`, `test_adversarial_m3.rs`, `test_adversarial_m3_challenger2.rs`, `test_adversarial_m3_challenger3.rs` |
| `ORIGINAL_REQUEST.md § R4` | `k0maru-router` CLI coordinator subcommands (`status`, `route`, `probe`, `outbox`) | Tested across `test_cli_coordinator.rs`, `test_adversarial_m4.rs`, `test_adversarial_m4_challenger2.rs` |
| `AGENTS.md § 6 & 7` | Highest observable testing seam, zero synthetic mock fallback, anti-corruption boundary | Real filesystem fixtures via `tempfile`, hermetic tmux socket isolation, zero synthetic mocks |
| `PROJECT.md § Milestones` | M1–M4 Feature completion -> M5 Test Ready Readiness Gate | All 27 inventory features mapped and verified 100% green |

// Integration tests for Issue #244: Unable to clear context pressure.
// Tests ledger reset, eviction, session reset clearing ledger,
// and actionable eviction hints.

mod ledger_reset {
    use lean_ctx::core::context_ledger::{ContextLedger, PressureAction};

    #[test]
    fn reset_clears_all_entries_and_totals() {
        let mut ledger = ContextLedger::with_window_size(10000);
        ledger.record("a.rs", "full", 3000, 3000);
        ledger.record("b.rs", "full", 3000, 3000);
        ledger.record("c.rs", "full", 3500, 3500);
        // 9500/10000 = 95% → must be EvictLeastRelevant (>90%)
        assert_eq!(
            ledger.pressure().recommendation,
            PressureAction::EvictLeastRelevant
        );

        ledger.reset();

        assert_eq!(ledger.entries.len(), 0);
        assert_eq!(ledger.total_tokens_sent, 0);
        assert_eq!(ledger.total_tokens_saved, 0);
        assert_eq!(ledger.pressure().recommendation, PressureAction::NoAction);
        assert!((ledger.pressure().utilization - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn reset_preserves_window_size() {
        let mut ledger = ContextLedger::with_window_size(200_000);
        ledger.record("big.rs", "full", 100_000, 100_000);
        ledger.reset();
        assert_eq!(ledger.window_size, 200_000);
    }
}

mod ledger_evict {
    use lean_ctx::core::context_ledger::ContextLedger;

    #[test]
    fn evict_removes_specific_paths() {
        let mut ledger = ContextLedger::with_window_size(10000);
        ledger.record("keep.rs", "full", 1000, 1000);
        ledger.record("evict_me.rs", "full", 5000, 5000);
        ledger.record("also_evict.rs", "full", 3000, 3000);

        let removed = ledger.evict_paths(&["evict_me.rs", "also_evict.rs"]);

        assert_eq!(removed, 2);
        assert_eq!(ledger.entries.len(), 1);
        assert_eq!(ledger.entries[0].path, "keep.rs");
        assert_eq!(ledger.total_tokens_sent, 1000);
    }

    #[test]
    fn evict_reduces_pressure() {
        let mut ledger = ContextLedger::with_window_size(10000);
        ledger.record("a.rs", "full", 5000, 5000);
        ledger.record("b.rs", "full", 5000, 5000);
        // 10000/10000 = 100% → EvictLeastRelevant
        assert!(ledger.pressure().utilization > 0.9);

        ledger.evict_paths(&["a.rs"]);

        // 5000/10000 = 50% → SuggestCompression
        assert!(
            ledger.pressure().utilization <= 0.5 + 0.05,
            "pressure should drop to ~50% after eviction, got: {:.1}%",
            ledger.pressure().utilization * 100.0,
        );
    }

    #[test]
    fn evict_nonexistent_path_is_noop() {
        let mut ledger = ContextLedger::with_window_size(10000);
        ledger.record("exists.rs", "full", 1000, 1000);

        let removed = ledger.evict_paths(&["nonexistent.rs", "also_missing.rs"]);

        assert_eq!(removed, 0);
        assert_eq!(ledger.entries.len(), 1);
        assert_eq!(ledger.total_tokens_sent, 1000);
    }

    #[test]
    fn evict_with_mixed_existing_and_missing() {
        let mut ledger = ContextLedger::with_window_size(10000);
        ledger.record("a.rs", "full", 1000, 1000);
        ledger.record("b.rs", "full", 2000, 2000);

        let removed = ledger.evict_paths(&["a.rs", "missing.rs", "b.rs"]);

        assert_eq!(removed, 2);
        assert_eq!(ledger.entries.len(), 0);
        assert_eq!(ledger.total_tokens_sent, 0);
    }
}

mod session_reset_clears_ledger {
    use lean_ctx::core::context_ledger::{ContextLedger, PressureAction};

    #[test]
    fn fresh_ledger_has_zero_pressure() {
        let ledger = ContextLedger::new();
        let pressure = ledger.pressure();
        assert!((pressure.utilization - 0.0).abs() < f64::EPSILON);
        assert_eq!(pressure.recommendation, PressureAction::NoAction);
    }

    #[test]
    fn simulated_session_reset_clears_pressure() {
        let mut ledger = ContextLedger::with_window_size(10000);
        ledger.record("file1.json", "full", 4000, 4000);
        ledger.record("file2.json", "full", 4000, 4000);
        ledger.record("script.py", "full", 3000, 3000);
        assert!(ledger.pressure().utilization > 0.9);

        // Simulate what session reset now does
        ledger.reset();

        assert_eq!(ledger.entries.len(), 0);
        assert_eq!(ledger.total_tokens_sent, 0);
        assert!((ledger.pressure().utilization - 0.0).abs() < f64::EPSILON);
    }
}

mod actionable_hints {
    use lean_ctx::core::context_ledger::ContextLedger;
    use lean_ctx::core::context_overlay::OverlayStore;
    use lean_ctx::server::context_gate;

    #[test]
    fn eviction_hint_contains_ctx_ledger_command() {
        let mut ledger = ContextLedger::with_window_size(1000);
        ledger.record("a.rs", "full", 300, 300);
        std::thread::sleep(std::time::Duration::from_millis(10));
        ledger.record("b.rs", "full", 300, 300);
        std::thread::sleep(std::time::Duration::from_millis(10));
        ledger.record("c.rs", "full", 300, 300);
        std::thread::sleep(std::time::Duration::from_millis(10));
        ledger.record("d.rs", "full", 200, 200);

        let overlay = OverlayStore::new();
        let result = context_gate::post_dispatch_record_with_task(
            "e.rs",
            "full",
            100,
            100,
            &mut ledger,
            &overlay,
            None,
        );

        if let Some(hint) = result.eviction_hint {
            assert!(
                hint.contains("ctx_ledger"),
                "hint should reference ctx_ledger tool: {hint}"
            );
            assert!(
                hint.contains("evict"),
                "hint should contain evict action: {hint}"
            );
        }
    }

    #[test]
    fn no_hint_at_low_pressure() {
        let mut ledger = ContextLedger::with_window_size(100_000);
        ledger.record("small.rs", "full", 100, 100);

        let overlay = OverlayStore::new();
        let result = context_gate::post_dispatch_record_with_task(
            "another.rs",
            "full",
            50,
            50,
            &mut ledger,
            &overlay,
            None,
        );

        assert!(
            result.eviction_hint.is_none(),
            "should not hint at low pressure"
        );
    }
}

mod double_booking_fix {
    use lean_ctx::core::context_ledger::ContextLedger;

    #[test]
    fn single_record_produces_correct_totals() {
        let mut ledger = ContextLedger::with_window_size(128_000);
        ledger.record("src/main.rs", "full", 5000, 3000);

        assert_eq!(ledger.entries.len(), 1);
        assert_eq!(ledger.total_tokens_sent, 3000);
        assert_eq!(ledger.total_tokens_saved, 2000);
    }

    #[test]
    fn upsert_same_path_does_not_double_count() {
        let mut ledger = ContextLedger::with_window_size(128_000);

        // First record (simulating dispatch)
        ledger.record("src/main.rs", "full", 5000, 5000);
        assert_eq!(ledger.total_tokens_sent, 5000);

        // Second record same path (simulating post_dispatch with different values)
        ledger.record("src/main.rs", "full", 5000, 3000);
        assert_eq!(ledger.entries.len(), 1);
        assert_eq!(ledger.total_tokens_sent, 3000);
        assert_eq!(ledger.total_tokens_saved, 2000);
    }

    #[test]
    fn remove_returns_bool_correctly() {
        let mut ledger = ContextLedger::with_window_size(10000);
        ledger.record("exists.rs", "full", 500, 500);

        assert!(ledger.remove("exists.rs"));
        assert!(!ledger.remove("exists.rs"));
        assert!(!ledger.remove("never_existed.rs"));
    }
}

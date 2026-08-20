/-
  LeanCTX Formal Verification — Secret Safety in Compression

  Proves that the aggressive compression mode never leaks secret patterns.
  This corresponds to the `io_boundary.rs` secret detection combined with
  context compilation policies.

  Mirrors: rust/src/core/io_boundary.rs, context_policies.rs
-/
import LeanCtxProofs.Basic

namespace LeanCtxProofs.Compression.SecretSafety

/-- A content line, possibly containing secrets. -/
structure ContentLine where
  text : String
  containsSecret : Bool
  deriving DecidableEq, Repr

/-- Aggressive mode filter: removes any line containing a secret pattern. -/
def aggressiveFilter (lines : List ContentLine) : List ContentLine :=
  lines.filter (! ·.containsSecret)

/-- Redacted mode: replaces secret content with a redaction marker. -/
def redactSecrets (lines : List ContentLine) : List ContentLine :=
  lines.map fun l =>
    if l.containsSecret then { l with text := "[REDACTED]", containsSecret := false }
    else l

-- ============================================================================
-- Secret Safety Theorems
-- ============================================================================

/-- **Theorem 1: Aggressive filter never outputs a line with a secret.** -/
theorem aggressive_no_secrets (lines : List ContentLine) :
    ∀ l ∈ aggressiveFilter lines, l.containsSecret = false := by
  intro l hl
  simp [aggressiveFilter, List.mem_filter] at hl
  simp [hl.2]

/-- **Theorem 2: Aggressive filter output is a subset of the input.** -/
theorem aggressive_subset (lines : List ContentLine) :
    ∀ l ∈ aggressiveFilter lines, l ∈ lines := by
  intro l hl
  simp [aggressiveFilter, List.mem_filter] at hl
  exact hl.1

/-- **Theorem 3: If input has no secrets, aggressive filter is identity.** -/
theorem aggressive_identity_when_clean (lines : List ContentLine)
    (h_clean : ∀ l ∈ lines, l.containsSecret = false) :
    aggressiveFilter lines = lines := by
  unfold aggressiveFilter
  rw [List.filter_eq_self]
  intro l hl
  simp [h_clean l hl]

/-- **Theorem 4: Redaction preserves line count.** -/
theorem redaction_preserves_length (lines : List ContentLine) :
    (redactSecrets lines).length = lines.length := by
  unfold redactSecrets
  exact List.length_map _ _

/-- **Theorem 5: After redaction, no line contains a secret.** -/
theorem redaction_clears_all_secrets (lines : List ContentLine) :
    ∀ l ∈ redactSecrets lines, l.containsSecret = false := by
  intro l hl
  unfold redactSecrets at hl
  rw [List.mem_map] at hl
  obtain ⟨orig, _, rfl⟩ := hl
  cases h_dec : orig.containsSecret <;> simp [h_dec]

/-- **Theorem 6: Non-secret lines are unchanged by redaction.** -/
theorem redaction_preserves_clean_lines (lines : List ContentLine) (l : ContentLine)
    (h_mem : l ∈ lines) (h_clean : l.containsSecret = false) :
    l ∈ redactSecrets lines := by
  unfold redactSecrets
  rw [List.mem_map]
  exact ⟨l, h_mem, by simp [h_clean]⟩

end LeanCtxProofs.Compression.SecretSafety

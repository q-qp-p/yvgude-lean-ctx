/-
  LeanCTX Formal Verification — Budget Enforcement Proofs

  Mirrors: rust/src/core/budget_tracker.rs
-/
import LeanCtxProofs.Basic

namespace LeanCtxProofs.Policy.BudgetEnforcement

structure DimensionState where
  used : Nat
  limit : Nat
  deriving DecidableEq, Repr

def percentUsed (state : DimensionState) : Nat :=
  if state.limit == 0 then 0
  else min 254 (state.used * 100 / state.limit)

def evaluateLevel (state : DimensionState) (config : BudgetConfig) : BudgetLevel :=
  let pct := percentUsed state
  if config.blockAtPercent < 255 ∧ pct ≥ config.blockAtPercent then
    .exhausted
  else if pct ≥ config.warnAtPercent then
    .warning
  else
    .ok

def recordUsage (state : DimensionState) (amount : Nat) : DimensionState :=
  { state with used := state.used + amount }

-- ============================================================================
-- Budget Theorems
-- ============================================================================

/-- When blocking is disabled (blockAtPercent = 255), never Exhausted. -/
theorem no_block_never_exhausted (state : DimensionState) (config : BudgetConfig)
    (h_noblock : config.blockAtPercent = 255) :
    evaluateLevel state config ≠ .exhausted := by
  unfold evaluateLevel
  simp only [h_noblock]
  simp
  split <;> simp

/-- Recording zero preserves the budget level. -/
theorem zero_record_preserves_level (state : DimensionState) (config : BudgetConfig) :
    evaluateLevel (recordUsage state 0) config = evaluateLevel state config := by
  unfold recordUsage
  simp

/-- percentUsed is bounded by 254. -/
theorem percent_bounded (state : DimensionState) :
    percentUsed state ≤ 254 := by
  unfold percentUsed
  split
  · omega
  · exact Nat.min_le_left 254 _

/-- Recording usage increases the used count. -/
theorem record_increases_used (state : DimensionState) (amount : Nat) :
    (recordUsage state amount).used = state.used + amount := by
  rfl

def defaultConfig : BudgetConfig :=
  { maxTokens := 200000, warnAtPercent := 80, blockAtPercent := 255 }

/-- With default config, budget is never exhausted. -/
theorem default_never_exhausted (state : DimensionState) :
    evaluateLevel state defaultConfig ≠ .exhausted :=
  no_block_never_exhausted state defaultConfig rfl

/-- If the level is exhausted, blocking must be explicitly enabled.
    Proof by case split on blockAtPercent. -/
theorem exhausted_means_blocking_enabled (state : DimensionState) (config : BudgetConfig)
    (h : evaluateLevel state config = .exhausted) :
    config.blockAtPercent < 255 := by
  unfold evaluateLevel at h
  simp only at h
  split at h
  · next hc => exact hc.1
  · split at h <;> simp at h

end LeanCtxProofs.Policy.BudgetEnforcement

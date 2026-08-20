/-
  LeanCTX Formal Verification — Scope Isolation Proofs

  Proves that agents can only access context items within their
  assigned scope.

  Mirrors: rust/src/server/role_guard.rs, http_server/team.rs
-/
import LeanCtxProofs.Basic

namespace LeanCtxProofs.Policy.ScopeIsolation

/-- Check if a path is within a scope (any allowed prefix matches). -/
def inScope (scope : Scope) (path : String) : Bool :=
  scope.allowedPrefixes.any (path.startsWith ·)

/-- Scope-guarded context ref expansion. Returns none if out of scope. -/
def expandRef (agent : Agent) (ref : ContextRef) (items : List ContextItem) :
    Option ContextItem :=
  if inScope agent.scope ref.itemId then
    items.find? (·.id == ref.itemId)
  else
    none

-- ============================================================================
-- Scope Isolation Theorems
-- ============================================================================

/-- **Theorem 1: An agent with empty scope cannot expand any ref.** -/
theorem empty_scope_blocks_all (agent : Agent) (ref : ContextRef) (items : List ContextItem)
    (h_empty : agent.scope.allowedPrefixes = []) :
    expandRef agent ref items = none := by
  unfold expandRef inScope
  simp [h_empty, List.any]

/-- **Theorem 2: Expansion only succeeds for in-scope refs.** -/
theorem expansion_requires_scope (agent : Agent) (ref : ContextRef) (items : List ContextItem)
    (h_out : inScope agent.scope ref.itemId = false) :
    expandRef agent ref items = none := by
  unfold expandRef
  simp [h_out]

/-- **Theorem 3: Scope with matching prefix grants access.** -/
theorem matching_prefix_grants_access (pfx : String) (path : String)
    (h : path.startsWith pfx = true) :
    inScope ⟨[pfx]⟩ path = true := by
  unfold inScope
  simp [List.any, h]

/-- **Theorem 4: Scope is prefix-monotone — adding prefixes can only expand access.** -/
theorem scope_monotone (scope : Scope) (newPfx : String) (path : String)
    (h : inScope scope path = true) :
    inScope { allowedPrefixes := newPfx :: scope.allowedPrefixes } path = true := by
  unfold inScope at *
  simp [List.any_cons, Bool.or_eq_true]
  simp [List.any_eq_true] at h
  exact Or.inr h

/-- **Theorem 5: If expansion returns Some, the item ID was in scope.** -/
theorem expansion_implies_in_scope (agent : Agent) (ref : ContextRef)
    (items : List ContextItem) (item : ContextItem)
    (h : expandRef agent ref items = some item) :
    inScope agent.scope ref.itemId = true := by
  unfold expandRef at h
  split at h <;> simp_all

end LeanCtxProofs.Policy.ScopeIsolation

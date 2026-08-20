/-
  LeanCTX Formal Verification — Context Governance Proofs

  Proves critical invariants of the context policy engine:
    1. excluded_items_never_rendered
    2. pinned_items_always_preserved
    3. setView preserves state

  Mirrors: rust/src/core/context_policies.rs, context_field.rs
  Methodology: Verification-Guided Development (arXiv:2407.01688)
-/
import LeanCtxProofs.Basic

namespace LeanCtxProofs.Policy.ContextGovernance

open ContextState PolicyAction

def applyAction (action : PolicyAction) (current : ContextState) (tokenCount : Nat) :
    ContextState :=
  match action with
  | .exclude => .excluded
  | .pin => .pinned
  | .include =>
    match current with
    | .candidate => .included
    | other => other
  | .markOutdated => .stale
  | .maxTokens limit =>
    if tokenCount > limit then .excluded else current
  | .setView _ => current

def isRenderable (s : ContextState) : Bool :=
  match s with
  | .excluded => false
  | .shadowed => false
  | _ => true

def compileContext (items : List ContextItem) : CompiledContext :=
  ⟨items.filter fun item => isRenderable item.state⟩

-- ============================================================================
-- Core Safety Theorems
-- ============================================================================

theorem excluded_items_never_rendered (items : List ContextItem) (item : ContextItem)
    (h_excl : item.state = ContextState.excluded) :
    item ∉ (compileContext items).items := by
  unfold compileContext
  simp [List.mem_filter]
  intro _
  simp [isRenderable, h_excl]

theorem pinned_items_always_preserved (items : List ContextItem) (item : ContextItem)
    (h_pin : item.state = ContextState.pinned)
    (h_mem : item ∈ items) :
    item ∈ (compileContext items).items := by
  unfold compileContext
  simp [List.mem_filter]
  exact ⟨h_mem, by simp [isRenderable, h_pin]⟩

theorem included_items_preserved (items : List ContextItem) (item : ContextItem)
    (h_incl : item.state = ContextState.included)
    (h_mem : item ∈ items) :
    item ∈ (compileContext items).items := by
  unfold compileContext
  simp [List.mem_filter]
  exact ⟨h_mem, by simp [isRenderable, h_incl]⟩

theorem exclude_action_always_excludes (state : ContextState) (tokenCount : Nat) :
    applyAction .exclude state tokenCount = .excluded := rfl

theorem pin_action_always_pins (state : ContextState) (tokenCount : Nat) :
    applyAction .pin state tokenCount = .pinned := rfl

theorem set_view_preserves_state (view : String) (state : ContextState) (tokenCount : Nat) :
    applyAction (.setView view) state tokenCount = state := rfl

theorem shadowed_items_never_rendered (items : List ContextItem) (item : ContextItem)
    (h_shadow : item.state = ContextState.shadowed) :
    item ∉ (compileContext items).items := by
  unfold compileContext
  simp [List.mem_filter]
  intro _
  simp [isRenderable, h_shadow]

theorem candidate_is_renderable : isRenderable ContextState.candidate = true := rfl
theorem stale_is_renderable : isRenderable ContextState.stale = true := rfl

/-- End-to-end: exclude action + compilation = item not in output. -/
theorem exclude_then_compile_removes (items : List ContextItem) (idx : Nat)
    (h_idx : idx < items.length)
    (h_excl : (items[idx]).state = ContextState.excluded) :
    items[idx] ∉ (compileContext items).items :=
  excluded_items_never_rendered items (items[idx]) h_excl

end LeanCtxProofs.Policy.ContextGovernance

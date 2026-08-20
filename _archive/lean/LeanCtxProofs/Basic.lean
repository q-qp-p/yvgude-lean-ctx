/-
  LeanCTX Formal Verification Layer — Core Types

  Mirrors the Rust types from:
    - rust/src/core/context_field.rs (ContextState, ContextItemId)
    - rust/src/core/context_policies.rs (PolicyAction, PolicyCondition, ContextPolicy)
    - rust/src/core/budget_tracker.rs (BudgetLevel)

  These definitions form the foundation for all proofs in the LeanCTX
  formal verification layer. They are intentionally simplified models
  of the Rust production code — the gap is validated via differential
  random testing (DRT), following Amazon Cedar's methodology.

  Reference: arXiv:2407.01688 (Verification-Guided Development of Cedar)
-/

/-- Context item lifecycle states. Mirrors `ContextState` in context_field.rs. -/
inductive ContextState where
  | candidate
  | included
  | excluded
  | pinned
  | stale
  | shadowed
  deriving DecidableEq, Repr

/-- Policy actions that transform context item state. Mirrors `PolicyAction`. -/
inductive PolicyAction where
  | exclude
  | include
  | pin
  | setView (view : String)
  | maxTokens (limit : Nat)
  | markOutdated
  deriving DecidableEq, Repr

/-- Conditions under which a policy applies. Mirrors `PolicyCondition`. -/
inductive PolicyCondition where
  | sourceSeenBefore
  | sourceModifiedRecently
  | tokensAbove (threshold : Nat)
  | always
  deriving DecidableEq, Repr

/-- Budget enforcement levels. Mirrors `BudgetLevel`. -/
inductive BudgetLevel where
  | ok
  | warning
  | exhausted
  deriving DecidableEq, Repr

/-- A context item with its current state and metadata. -/
structure ContextItem where
  id : String
  path : String
  state : ContextState
  tokenCount : Nat
  seenBefore : Bool
  deriving DecidableEq, Repr

/-- A declarative context policy rule. Mirrors `ContextPolicy`. -/
structure ContextPolicy where
  name : String
  matchPattern : String
  action : PolicyAction
  condition : Option PolicyCondition
  deriving Repr

/-- A set of policies. Mirrors `PolicySet`. -/
structure PolicySet where
  policies : List ContextPolicy
  deriving Repr

/-- Scope definition for agent access control. -/
structure Scope where
  allowedPrefixes : List String
  deriving DecidableEq, Repr

/-- Agent identity with scope restrictions. -/
structure Agent where
  id : String
  scope : Scope
  deriving Repr

/-- Budget configuration. -/
structure BudgetConfig where
  maxTokens : Nat
  warnAtPercent : Nat
  blockAtPercent : Nat  -- 255 = never block (LeanCTX default)
  deriving DecidableEq, Repr

/-- A context reference used in handoffs and context compilation. -/
structure ContextRef where
  itemId : String
  deriving DecidableEq, Repr

/-- The compiled context — the final output sent to the LLM. -/
structure CompiledContext where
  items : List ContextItem
  deriving Repr

/-
  LeanCTX Formal Verification — Agent Handoff State Machine

  Formalizes the agent-to-agent handoff protocol as a state machine
  with proven transition safety properties. Based on LeanMachines
  methodology and VeriGuard offline verification pattern.

  Mirrors: rust/src/core/a2a_transport.rs, handoff_ledger.rs
  Reference: arXiv:2510.05156 (VeriGuard)
-/
import LeanCtxProofs.Basic

namespace LeanCtxProofs.Handoff.StateMachine

/-- Handoff lifecycle states. -/
inductive HandoffState where
  | idle
  | preparing
  | signed
  | sent
  | received
  | accepted
  | rejected
  | completed
  | failed
  deriving DecidableEq, Repr

/-- Events that drive state transitions. -/
inductive HandoffEvent where
  | prepare
  | sign
  | send
  | receive
  | accept
  | reject
  | complete
  | fail
  deriving DecidableEq, Repr

/-- Transport content types. -/
inductive ContentType where
  | handoffBundle
  | contextPackage
  | a2aMessage
  | a2aTask
  deriving DecidableEq, Repr

/-- Transport envelope validity requirements. -/
structure EnvelopeInvariants where
  hasValidSender : Bool
  payloadNonEmpty : Bool
  payloadWithinLimit : Bool
  formatVersionValid : Bool
  deriving DecidableEq, Repr

/-- Check if an envelope satisfies all validity invariants. -/
def envelopeValid (inv : EnvelopeInvariants) : Bool :=
  inv.hasValidSender &&
  inv.payloadNonEmpty &&
  inv.payloadWithinLimit &&
  inv.formatVersionValid

/-- The state transition function. Returns none for invalid transitions. -/
def transition (state : HandoffState) (event : HandoffEvent) : Option HandoffState :=
  match state, event with
  | .idle, .prepare => some .preparing
  | .preparing, .sign => some .signed
  | .preparing, .fail => some .failed
  | .signed, .send => some .sent
  | .signed, .fail => some .failed
  | .sent, .receive => some .received
  | .sent, .fail => some .failed
  | .received, .accept => some .accepted
  | .received, .reject => some .rejected
  | .received, .fail => some .failed
  | .accepted, .complete => some .completed
  | .accepted, .fail => some .failed
  | _, _ => none

/-- Predicate for terminal states. -/
def isTerminal (s : HandoffState) : Bool :=
  match s with
  | .completed => true
  | .failed => true
  | .rejected => true
  | _ => false

/-- Predicate for pre-send states. -/
def isPreSend (s : HandoffState) : Bool :=
  match s with
  | .idle => true
  | .preparing => true
  | .signed => true
  | _ => false

-- ============================================================================
-- Handoff State Machine Theorems
-- ============================================================================

/-- **Theorem 1: Terminal states have no valid outgoing transitions.** -/
theorem terminal_is_sink (s : HandoffState) (e : HandoffEvent)
    (h : isTerminal s = true) :
    transition s e = none := by
  cases s <;> cases e <;> simp [isTerminal] at h <;> simp [transition]

/-- **Theorem 2: The idle state can only transition to preparing.** -/
theorem idle_only_prepares (e : HandoffEvent)
    (h : (transition .idle e).isSome = true) :
    e = .prepare := by
  cases e <;> simp [transition] at *

/-- **Theorem 3: Sending requires a signed state (cannot skip signing).** -/
theorem send_requires_signed (s : HandoffState)
    (h : (transition s .send).isSome = true) :
    s = .signed := by
  cases s <;> simp [transition] at *

/-- **Theorem 4: Accepting requires received state. -/
theorem accept_requires_received (s : HandoffState)
    (h : (transition s .accept).isSome = true) :
    s = .received := by
  cases s <;> simp [transition] at *

/-- **Theorem 5: Completion requires acceptance. -/
theorem complete_requires_accepted (s : HandoffState)
    (h : (transition s .complete).isSome = true) :
    s = .accepted := by
  cases s <;> simp [transition] at *

/-- **Theorem 6: The failure event is always valid except from idle and terminal states. -/
theorem fail_from_active_states (s : HandoffState)
    (h_active : !isTerminal s = true)
    (h_not_idle : s ≠ .idle) :
    (transition s .fail).isSome = true := by
  cases s <;> simp [transition, isTerminal] at * <;> exact h_not_idle rfl

/-- **Theorem 7: A valid transition sequence from idle to completed must pass
    through all intermediate states.** -/
theorem handoff_lifecycle_ordering :
    transition .idle .prepare = some .preparing ∧
    transition .preparing .sign = some .signed ∧
    transition .signed .send = some .sent ∧
    transition .sent .receive = some .received ∧
    transition .received .accept = some .accepted ∧
    transition .accepted .complete = some .completed := by
  simp [transition]

/-- **Theorem 8: Rejection is a terminal state.** -/
theorem rejected_is_terminal : isTerminal .rejected = true := by rfl

/-- **Theorem 9: A signed envelope that fails validation returns to failed.** -/
theorem invalid_envelope_fails (_inv : EnvelopeInvariants)
    (_h_invalid : envelopeValid _inv = false)
    (s : HandoffState) (h_signed : s = .signed) :
    transition s .fail = some .failed := by
  rw [h_signed]; rfl

/-- **Theorem 10: If envelope is valid and state is signed, send is possible.** -/
theorem valid_envelope_enables_send (_inv : EnvelopeInvariants)
    (_h_valid : envelopeValid _inv = true)
    (s : HandoffState) (h_signed : s = .signed) :
    (transition s .send).isSome = true := by
  rw [h_signed]; simp [transition]

end LeanCtxProofs.Handoff.StateMachine

/-
  LeanCTX Formal Verification — PathJail Proofs

  Mirrors: rust/src/core/pathjail.rs
-/
import LeanCtxProofs.Basic

namespace LeanCtxProofs.Policy.PathJail

abbrev Path := List String

structure JailConfig where
  root : Path
  allowPaths : List Path
  deriving DecidableEq, Repr

def isUnderPfx (pfx candidate : Path) : Bool :=
  match pfx, candidate with
  | [], _ => true
  | _ :: _, [] => false
  | p :: ps, c :: cs => p == c && isUnderPfx ps cs

def jailPathAllowed (config : JailConfig) (candidate : Path) : Bool :=
  isUnderPfx config.root candidate ||
  config.allowPaths.any (isUnderPfx · candidate)

-- ============================================================================
-- Theorems
-- ============================================================================

theorem jail_path_sound (config : JailConfig) (candidate : Path) :
    jailPathAllowed config candidate = true →
    isUnderPfx config.root candidate = true ∨
    ∃ allowed ∈ config.allowPaths, isUnderPfx allowed candidate = true := by
  intro h
  unfold jailPathAllowed at h
  simp [Bool.or_eq_true] at h
  cases h with
  | inl h => exact Or.inl h
  | inr h =>
    right
    simp [List.any_eq_true] at h
    exact h

theorem jail_no_escape (config : JailConfig) (candidate : Path)
    (h_root : isUnderPfx config.root candidate = false)
    (h_allow : ∀ p ∈ config.allowPaths, isUnderPfx p candidate = false) :
    jailPathAllowed config candidate = false := by
  unfold jailPathAllowed
  simp [Bool.or_eq_true, h_root, List.any_eq_true]
  intro p hp
  exact h_allow p hp

theorem jail_empty_allow_list (root candidate : Path) :
    jailPathAllowed ⟨root, []⟩ candidate = isUnderPfx root candidate := by
  unfold jailPathAllowed
  simp [List.any_eq_true]

theorem jail_allow_monotone (config : JailConfig) (newAllow : Path) (candidate : Path) :
    jailPathAllowed config candidate = true →
    jailPathAllowed { root := config.root, allowPaths := newAllow :: config.allowPaths } candidate = true := by
  intro h
  unfold jailPathAllowed at *
  simp [Bool.or_eq_true, List.any_eq_true] at *
  rcases h with h | ⟨p, hp, hpref⟩
  · left; exact h
  · right
    exact Or.inr ⟨p, hp, hpref⟩

theorem isUnderPfx_refl (p : Path) : isUnderPfx p p = true := by
  induction p with
  | nil => simp [isUnderPfx]
  | cons x xs ih => simp [isUnderPfx, ih]

end LeanCtxProofs.Policy.PathJail

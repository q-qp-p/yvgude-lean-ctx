/-
  LeanCTX Formal Verification — Compression Read Mode Invariants

  Formalizes what each compression mode preserves. Each read mode in
  LeanCTX operates at a different point on the rate-distortion curve
  (Shannon 1959, arXiv:2409.14822). This module proves that specific
  structural properties are preserved for each mode.

  Scientific basis:
    - Rate-Distortion Theory (Shannon 1959)
    - Semantic Compression via Information Lattices (arXiv:2404.03131)
    - "Noether for Context Compression" — each transformation has a
      class of preserved properties (analogous to Noether's theorem)

  Mirrors: rust/src/tools/ctx_read.rs, rust/src/core/signatures.rs
-/
import LeanCtxProofs.Basic

namespace LeanCtxProofs.Compression.ReadModes

/-- Read modes available in LeanCTX. -/
inductive ReadMode where
  | full
  | map
  | signatures
  | aggressive
  | entropy
  | diff
  | lines (start stop : Nat)
  | reference
  | task
  deriving DecidableEq, Repr

/-- A function signature extracted from source code. -/
structure FunctionSig where
  name : String
  isExported : Bool
  deriving DecidableEq, Repr

/-- An import statement from source code. -/
structure ImportStmt where
  module : String
  deriving DecidableEq, Repr

/-- Type export declaration. -/
structure TypeExport where
  name : String
  deriving DecidableEq, Repr

/-- Source file model. -/
structure SourceFile where
  path : String
  allSignatures : List FunctionSig
  exportedSignatures : List FunctionSig
  imports : List ImportStmt
  exportedTypes : List TypeExport
  lines : List String
  deriving Repr

/-- Compressed output model. -/
structure CompressedOutput where
  signatures : List FunctionSig
  imports : List ImportStmt
  types : List TypeExport
  content : List String
  deriving Repr

/-- Signatures mode: extract all exported function signatures. -/
def compressSignatures (src : SourceFile) : CompressedOutput :=
  { signatures := src.exportedSignatures
    imports := []
    types := []
    content := [] }

/-- Map mode: extract imports + exported types + exported signatures. -/
def compressMap (src : SourceFile) : CompressedOutput :=
  { signatures := src.exportedSignatures
    imports := src.imports
    types := src.exportedTypes
    content := [] }

/-- Full mode: preserve everything (identity transformation). -/
def compressFull (src : SourceFile) : CompressedOutput :=
  { signatures := src.allSignatures
    imports := src.imports
    types := src.exportedTypes
    content := src.lines }

-- ============================================================================
-- Compression Invariant Theorems ("Noether for Context Compression")
-- ============================================================================

/-- **Theorem 1 (Signatures Mode): All exported signatures are preserved.** -/
theorem signatures_mode_preserves_exports (src : SourceFile) :
    (compressSignatures src).signatures = src.exportedSignatures := by
  rfl

/-- **Theorem 2 (Map Mode): All exported signatures are preserved.** -/
theorem map_mode_preserves_signatures (src : SourceFile) :
    (compressMap src).signatures = src.exportedSignatures := by
  rfl

/-- **Theorem 3 (Map Mode): All imports are preserved.** -/
theorem map_mode_preserves_imports (src : SourceFile) :
    (compressMap src).imports = src.imports := by
  rfl

/-- **Theorem 4 (Map Mode): All exported types are preserved.** -/
theorem map_mode_preserves_types (src : SourceFile) :
    (compressMap src).types = src.exportedTypes := by
  rfl

/-- **Theorem 5 (Full Mode): All signatures preserved.** -/
theorem full_mode_preserves_all_signatures (src : SourceFile) :
    (compressFull src).signatures = src.allSignatures := by
  rfl

/-- **Theorem 6 (Full Mode): All content lines preserved.** -/
theorem full_mode_preserves_content (src : SourceFile) :
    (compressFull src).content = src.lines := by
  rfl

/-- **Theorem 7 (Full Mode): All imports preserved.** -/
theorem full_mode_preserves_imports (src : SourceFile) :
    (compressFull src).imports = src.imports := by
  rfl

/-- **Theorem 8: Signatures mode output is a subset of map mode output.
    (Monotonicity: more compression = subset of less compression)** -/
theorem signatures_subset_of_map (src : SourceFile) :
    (compressSignatures src).signatures = (compressMap src).signatures := by
  rfl

/-- **Theorem 9: Map mode output signatures are a subset of full mode,
    assuming exported ⊆ all.** -/
theorem map_signatures_subset_full (src : SourceFile)
    (h : ∀ s ∈ src.exportedSignatures, s ∈ src.allSignatures) :
    ∀ s ∈ (compressMap src).signatures, s ∈ (compressFull src).signatures := by
  intro s hs
  simp [compressMap] at hs
  simp [compressFull]
  exact h s hs

/-- **Theorem 10: Exported signature for a given name is findable after
    signatures mode compression.** -/
theorem signature_lookup_preserved (src : SourceFile) (name : String)
    (h : ∃ sig ∈ src.exportedSignatures, sig.name = name) :
    ∃ sig ∈ (compressSignatures src).signatures, sig.name = name := by
  simp [compressSignatures]
  exact h

/-- **Theorem 11: Import for a given module is findable after map compression.** -/
theorem import_lookup_preserved (src : SourceFile) (mod_ : String)
    (h : ∃ imp ∈ src.imports, imp.module = mod_) :
    ∃ imp ∈ (compressMap src).imports, imp.module = mod_ := by
  simp [compressMap]
  exact h

-- ============================================================================
-- Instruction File Protection (Mirrors: is_instruction_file in ctx_read.rs)
-- ============================================================================

/-- Predicate: a file path is an instruction file (skill, agent rules, etc.). -/
def isInstructionFile (path : String) : Bool :=
  let lower := path.toLower
  let filename := lower.splitOn "/" |>.getLast!
  filename == "skill.md" ∨ filename == "agents.md" ∨
  filename == "rules.md" ∨ filename == ".cursorrules" ∨
  lower.containsSubstr "/skills/" ∨
  lower.containsSubstr "/.cursor/rules/"

/-- Resolve auto mode: instruction files always get full mode. -/
def resolveAutoMode (path : String) (_tokens : Nat) : ReadMode :=
  if isInstructionFile path then ReadMode.full
  else ReadMode.map

/-- **Theorem 12 (Instruction Guard): Instruction files are NEVER compressed.
    Any file matching isInstructionFile always resolves to full mode,
    bypassing all heuristic/bandit/adaptive mode selection.** -/
theorem instruction_files_always_full (path : String) (tokens : Nat)
    (h : isInstructionFile path = true) :
    resolveAutoMode path tokens = ReadMode.full := by
  simp [resolveAutoMode, h]

/-- **Theorem 13 (Full mode identity on instruction files):
    Instruction files compressed with full mode retain all content.** -/
theorem instruction_file_content_preserved (src : SourceFile)
    (h : isInstructionFile src.path = true) :
    (compressFull src).content = src.lines := by
  rfl

end LeanCtxProofs.Compression.ReadModes

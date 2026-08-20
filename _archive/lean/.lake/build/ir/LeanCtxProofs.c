// Lean compiler output
// Module: LeanCtxProofs
// Imports: Init LeanCtxProofs.Basic LeanCtxProofs.Policy.PathJail LeanCtxProofs.Policy.ContextGovernance LeanCtxProofs.Policy.BudgetEnforcement LeanCtxProofs.Policy.ScopeIsolation LeanCtxProofs.Compression.ReadModes LeanCtxProofs.Compression.SecretSafety LeanCtxProofs.Compression.TerseQuality LeanCtxProofs.Compression.TerseEngine LeanCtxProofs.Handoff.StateMachine
#include <lean/lean.h>
#if defined(__clang__)
#pragma clang diagnostic ignored "-Wunused-parameter"
#pragma clang diagnostic ignored "-Wunused-label"
#elif defined(__GNUC__) && !defined(__CLANG__)
#pragma GCC diagnostic ignored "-Wunused-parameter"
#pragma GCC diagnostic ignored "-Wunused-label"
#pragma GCC diagnostic ignored "-Wunused-but-set-variable"
#endif
#ifdef __cplusplus
extern "C" {
#endif
lean_object* initialize_Init(uint8_t builtin, lean_object*);
lean_object* initialize_LeanCtxProofs_Basic(uint8_t builtin, lean_object*);
lean_object* initialize_LeanCtxProofs_Policy_PathJail(uint8_t builtin, lean_object*);
lean_object* initialize_LeanCtxProofs_Policy_ContextGovernance(uint8_t builtin, lean_object*);
lean_object* initialize_LeanCtxProofs_Policy_BudgetEnforcement(uint8_t builtin, lean_object*);
lean_object* initialize_LeanCtxProofs_Policy_ScopeIsolation(uint8_t builtin, lean_object*);
lean_object* initialize_LeanCtxProofs_Compression_ReadModes(uint8_t builtin, lean_object*);
lean_object* initialize_LeanCtxProofs_Compression_SecretSafety(uint8_t builtin, lean_object*);
lean_object* initialize_LeanCtxProofs_Compression_TerseQuality(uint8_t builtin, lean_object*);
lean_object* initialize_LeanCtxProofs_Compression_TerseEngine(uint8_t builtin, lean_object*);
lean_object* initialize_LeanCtxProofs_Handoff_StateMachine(uint8_t builtin, lean_object*);
static bool _G_initialized = false;
LEAN_EXPORT lean_object* initialize_LeanCtxProofs(uint8_t builtin, lean_object* w) {
lean_object * res;
if (_G_initialized) return lean_io_result_mk_ok(lean_box(0));
_G_initialized = true;
res = initialize_Init(builtin, lean_io_mk_world());
if (lean_io_result_is_error(res)) return res;
lean_dec_ref(res);
res = initialize_LeanCtxProofs_Basic(builtin, lean_io_mk_world());
if (lean_io_result_is_error(res)) return res;
lean_dec_ref(res);
res = initialize_LeanCtxProofs_Policy_PathJail(builtin, lean_io_mk_world());
if (lean_io_result_is_error(res)) return res;
lean_dec_ref(res);
res = initialize_LeanCtxProofs_Policy_ContextGovernance(builtin, lean_io_mk_world());
if (lean_io_result_is_error(res)) return res;
lean_dec_ref(res);
res = initialize_LeanCtxProofs_Policy_BudgetEnforcement(builtin, lean_io_mk_world());
if (lean_io_result_is_error(res)) return res;
lean_dec_ref(res);
res = initialize_LeanCtxProofs_Policy_ScopeIsolation(builtin, lean_io_mk_world());
if (lean_io_result_is_error(res)) return res;
lean_dec_ref(res);
res = initialize_LeanCtxProofs_Compression_ReadModes(builtin, lean_io_mk_world());
if (lean_io_result_is_error(res)) return res;
lean_dec_ref(res);
res = initialize_LeanCtxProofs_Compression_SecretSafety(builtin, lean_io_mk_world());
if (lean_io_result_is_error(res)) return res;
lean_dec_ref(res);
res = initialize_LeanCtxProofs_Compression_TerseQuality(builtin, lean_io_mk_world());
if (lean_io_result_is_error(res)) return res;
lean_dec_ref(res);
res = initialize_LeanCtxProofs_Compression_TerseEngine(builtin, lean_io_mk_world());
if (lean_io_result_is_error(res)) return res;
lean_dec_ref(res);
res = initialize_LeanCtxProofs_Handoff_StateMachine(builtin, lean_io_mk_world());
if (lean_io_result_is_error(res)) return res;
lean_dec_ref(res);
return lean_io_result_mk_ok(lean_box(0));
}
#ifdef __cplusplus
}
#endif

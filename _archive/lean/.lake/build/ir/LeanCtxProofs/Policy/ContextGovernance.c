// Lean compiler output
// Module: LeanCtxProofs.Policy.ContextGovernance
// Imports: Init LeanCtxProofs.Basic
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
LEAN_EXPORT uint8_t l_LeanCtxProofs_Policy_ContextGovernance_isRenderable(uint8_t);
LEAN_EXPORT uint8_t l_LeanCtxProofs_Policy_ContextGovernance_applyAction(lean_object*, uint8_t, lean_object*);
LEAN_EXPORT lean_object* l_LeanCtxProofs_Policy_ContextGovernance_compileContext(lean_object*);
LEAN_EXPORT lean_object* l___private_LeanCtxProofs_Policy_ContextGovernance_0__LeanCtxProofs_Policy_ContextGovernance_isRenderable_match__1_splitter___rarg(uint8_t, lean_object*, lean_object*, lean_object*);
LEAN_EXPORT lean_object* l_LeanCtxProofs_Policy_ContextGovernance_applyAction___boxed(lean_object*, lean_object*, lean_object*);
LEAN_EXPORT lean_object* l_LeanCtxProofs_Policy_ContextGovernance_isRenderable___boxed(lean_object*);
uint8_t lean_nat_dec_lt(lean_object*, lean_object*);
LEAN_EXPORT lean_object* l_List_filterTR_loop___at_LeanCtxProofs_Policy_ContextGovernance_compileContext___spec__1(lean_object*, lean_object*);
LEAN_EXPORT lean_object* l___private_LeanCtxProofs_Policy_ContextGovernance_0__LeanCtxProofs_Policy_ContextGovernance_isRenderable_match__1_splitter(lean_object*);
lean_object* l_List_reverse___rarg(lean_object*);
LEAN_EXPORT lean_object* l___private_LeanCtxProofs_Policy_ContextGovernance_0__LeanCtxProofs_Policy_ContextGovernance_isRenderable_match__1_splitter___rarg___boxed(lean_object*, lean_object*, lean_object*, lean_object*);
LEAN_EXPORT uint8_t l_LeanCtxProofs_Policy_ContextGovernance_applyAction(lean_object* x_1, uint8_t x_2, lean_object* x_3) {
_start:
{
switch (lean_obj_tag(x_1)) {
case 0:
{
uint8_t x_4; 
x_4 = 2;
return x_4;
}
case 1:
{
lean_object* x_5; 
x_5 = lean_box(x_2);
if (lean_obj_tag(x_5) == 0)
{
uint8_t x_6; 
x_6 = 1;
return x_6;
}
else
{
lean_dec(x_5);
return x_2;
}
}
case 2:
{
uint8_t x_7; 
x_7 = 3;
return x_7;
}
case 3:
{
return x_2;
}
case 4:
{
lean_object* x_8; uint8_t x_9; 
x_8 = lean_ctor_get(x_1, 0);
x_9 = lean_nat_dec_lt(x_8, x_3);
if (x_9 == 0)
{
return x_2;
}
else
{
uint8_t x_10; 
x_10 = 2;
return x_10;
}
}
default: 
{
uint8_t x_11; 
x_11 = 4;
return x_11;
}
}
}
}
LEAN_EXPORT lean_object* l_LeanCtxProofs_Policy_ContextGovernance_applyAction___boxed(lean_object* x_1, lean_object* x_2, lean_object* x_3) {
_start:
{
uint8_t x_4; uint8_t x_5; lean_object* x_6; 
x_4 = lean_unbox(x_2);
lean_dec(x_2);
x_5 = l_LeanCtxProofs_Policy_ContextGovernance_applyAction(x_1, x_4, x_3);
lean_dec(x_3);
lean_dec(x_1);
x_6 = lean_box(x_5);
return x_6;
}
}
LEAN_EXPORT uint8_t l_LeanCtxProofs_Policy_ContextGovernance_isRenderable(uint8_t x_1) {
_start:
{
lean_object* x_2; 
x_2 = lean_box(x_1);
switch (lean_obj_tag(x_2)) {
case 2:
{
uint8_t x_3; 
x_3 = 0;
return x_3;
}
case 5:
{
uint8_t x_4; 
x_4 = 0;
return x_4;
}
default: 
{
uint8_t x_5; 
lean_dec(x_2);
x_5 = 1;
return x_5;
}
}
}
}
LEAN_EXPORT lean_object* l_LeanCtxProofs_Policy_ContextGovernance_isRenderable___boxed(lean_object* x_1) {
_start:
{
uint8_t x_2; uint8_t x_3; lean_object* x_4; 
x_2 = lean_unbox(x_1);
lean_dec(x_1);
x_3 = l_LeanCtxProofs_Policy_ContextGovernance_isRenderable(x_2);
x_4 = lean_box(x_3);
return x_4;
}
}
LEAN_EXPORT lean_object* l_List_filterTR_loop___at_LeanCtxProofs_Policy_ContextGovernance_compileContext___spec__1(lean_object* x_1, lean_object* x_2) {
_start:
{
if (lean_obj_tag(x_1) == 0)
{
lean_object* x_3; 
x_3 = l_List_reverse___rarg(x_2);
return x_3;
}
else
{
uint8_t x_4; 
x_4 = !lean_is_exclusive(x_1);
if (x_4 == 0)
{
lean_object* x_5; lean_object* x_6; uint8_t x_7; uint8_t x_8; 
x_5 = lean_ctor_get(x_1, 0);
x_6 = lean_ctor_get(x_1, 1);
x_7 = lean_ctor_get_uint8(x_5, sizeof(void*)*3);
x_8 = l_LeanCtxProofs_Policy_ContextGovernance_isRenderable(x_7);
if (x_8 == 0)
{
lean_free_object(x_1);
lean_dec(x_5);
x_1 = x_6;
goto _start;
}
else
{
lean_ctor_set(x_1, 1, x_2);
{
lean_object* _tmp_0 = x_6;
lean_object* _tmp_1 = x_1;
x_1 = _tmp_0;
x_2 = _tmp_1;
}
goto _start;
}
}
else
{
lean_object* x_11; lean_object* x_12; uint8_t x_13; uint8_t x_14; 
x_11 = lean_ctor_get(x_1, 0);
x_12 = lean_ctor_get(x_1, 1);
lean_inc(x_12);
lean_inc(x_11);
lean_dec(x_1);
x_13 = lean_ctor_get_uint8(x_11, sizeof(void*)*3);
x_14 = l_LeanCtxProofs_Policy_ContextGovernance_isRenderable(x_13);
if (x_14 == 0)
{
lean_dec(x_11);
x_1 = x_12;
goto _start;
}
else
{
lean_object* x_16; 
x_16 = lean_alloc_ctor(1, 2, 0);
lean_ctor_set(x_16, 0, x_11);
lean_ctor_set(x_16, 1, x_2);
x_1 = x_12;
x_2 = x_16;
goto _start;
}
}
}
}
}
LEAN_EXPORT lean_object* l_LeanCtxProofs_Policy_ContextGovernance_compileContext(lean_object* x_1) {
_start:
{
lean_object* x_2; lean_object* x_3; 
x_2 = lean_box(0);
x_3 = l_List_filterTR_loop___at_LeanCtxProofs_Policy_ContextGovernance_compileContext___spec__1(x_1, x_2);
return x_3;
}
}
LEAN_EXPORT lean_object* l___private_LeanCtxProofs_Policy_ContextGovernance_0__LeanCtxProofs_Policy_ContextGovernance_isRenderable_match__1_splitter___rarg(uint8_t x_1, lean_object* x_2, lean_object* x_3, lean_object* x_4) {
_start:
{
lean_object* x_5; 
x_5 = lean_box(x_1);
switch (lean_obj_tag(x_5)) {
case 2:
{
lean_dec(x_4);
lean_inc(x_2);
return x_2;
}
case 5:
{
lean_dec(x_4);
lean_inc(x_3);
return x_3;
}
default: 
{
lean_object* x_6; lean_object* x_7; 
lean_dec(x_5);
x_6 = lean_box(x_1);
x_7 = lean_apply_3(x_4, x_6, lean_box(0), lean_box(0));
return x_7;
}
}
}
}
LEAN_EXPORT lean_object* l___private_LeanCtxProofs_Policy_ContextGovernance_0__LeanCtxProofs_Policy_ContextGovernance_isRenderable_match__1_splitter(lean_object* x_1) {
_start:
{
lean_object* x_2; 
x_2 = lean_alloc_closure((void*)(l___private_LeanCtxProofs_Policy_ContextGovernance_0__LeanCtxProofs_Policy_ContextGovernance_isRenderable_match__1_splitter___rarg___boxed), 4, 0);
return x_2;
}
}
LEAN_EXPORT lean_object* l___private_LeanCtxProofs_Policy_ContextGovernance_0__LeanCtxProofs_Policy_ContextGovernance_isRenderable_match__1_splitter___rarg___boxed(lean_object* x_1, lean_object* x_2, lean_object* x_3, lean_object* x_4) {
_start:
{
uint8_t x_5; lean_object* x_6; 
x_5 = lean_unbox(x_1);
lean_dec(x_1);
x_6 = l___private_LeanCtxProofs_Policy_ContextGovernance_0__LeanCtxProofs_Policy_ContextGovernance_isRenderable_match__1_splitter___rarg(x_5, x_2, x_3, x_4);
lean_dec(x_3);
lean_dec(x_2);
return x_6;
}
}
lean_object* initialize_Init(uint8_t builtin, lean_object*);
lean_object* initialize_LeanCtxProofs_Basic(uint8_t builtin, lean_object*);
static bool _G_initialized = false;
LEAN_EXPORT lean_object* initialize_LeanCtxProofs_Policy_ContextGovernance(uint8_t builtin, lean_object* w) {
lean_object * res;
if (_G_initialized) return lean_io_result_mk_ok(lean_box(0));
_G_initialized = true;
res = initialize_Init(builtin, lean_io_mk_world());
if (lean_io_result_is_error(res)) return res;
lean_dec_ref(res);
res = initialize_LeanCtxProofs_Basic(builtin, lean_io_mk_world());
if (lean_io_result_is_error(res)) return res;
lean_dec_ref(res);
return lean_io_result_mk_ok(lean_box(0));
}
#ifdef __cplusplus
}
#endif

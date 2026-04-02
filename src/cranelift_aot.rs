use cranelift::prelude::*;
use cranelift_object::{ObjectBuilder, ObjectModule};
use cranelift_module::{Module, Linkage, FuncId};
use std::collections::HashMap;
use tryzub_parser::*;

pub fn compile_to_object(program: &Program) -> anyhow::Result<Vec<u8>> {
    let mut flag_builder = settings::builder();
    flag_builder.set("is_pic", "false").unwrap();
    let isa_builder = cranelift_native::builder().unwrap_or_else(|msg| {
        panic!("host ISA: {}", msg);
    });
    let isa = isa_builder.finish(settings::Flags::new(flag_builder)).unwrap();
    let obj_builder = ObjectBuilder::new(isa, "tryzub_program", cranelift_module::default_libcall_names()).unwrap();
    let mut module = ObjectModule::new(obj_builder);
    let mut builder_ctx = FunctionBuilderContext::new();
    let mut ctx = module.make_context();
    let mut functions: HashMap<String, FuncId> = HashMap::new();

    let mut func_decls: Vec<(String, Vec<String>, Vec<Statement>)> = Vec::new();
    for decl in &program.declarations {
        if let Declaration::Function { name, params, body, .. } = decl {
            func_decls.push((name.clone(), params.iter().map(|p| p.name.clone()).collect(), body.clone()));
        }
    }

    for (name, params, _) in &func_decls {
        let mut sig = module.make_signature();
        for _ in params { sig.params.push(AbiParam::new(types::I64)); }
        sig.returns.push(AbiParam::new(types::I64));
        let fid = module.declare_function(name, Linkage::Local, &sig)?;
        functions.insert(name.clone(), fid);
    }

    let rt = [
        ("__tryzub_print", &[types::I64][..]),
        ("__tryzub_print_f64", &[types::F64]),
        ("__tryzub_print_str", &[types::I64, types::I64]),
        ("__tryzub_concat", &[types::I64, types::I64]),
        ("__tryzub_array_new", &[types::I64]),
        ("__tryzub_array_push", &[types::I64, types::I64]),
        ("__tryzub_array_get", &[types::I64, types::I64]),
        ("__tryzub_array_set", &[types::I64, types::I64, types::I64]),
        ("__tryzub_array_len", &[types::I64]),
        ("__tryzub_format_int", &[types::I64]),
        ("__tryzub_format_f64", &[types::F64]),
    ];
    let names = [
        ("__tryzub_print", "друк"), ("__tryzub_print_f64", "__друк_дрб"),
        ("__tryzub_print_str", "__друк_рядок"), ("__tryzub_concat", "__concat"),
        ("__tryzub_array_new", "__array_new"), ("__tryzub_array_push", "__array_push"),
        ("__tryzub_array_get", "__array_get"), ("__tryzub_array_set", "__array_set"),
        ("__tryzub_array_len", "__array_len"), ("__tryzub_format_int", "__format_int"),
        ("__tryzub_format_f64", "__format_f64"),
    ];
    let name_map: HashMap<&str, &str> = names.into_iter().collect();

    for (ext, params) in &rt {
        let mut sig = module.make_signature();
        for p in *params { sig.params.push(AbiParam::new(*p)); }
        sig.returns.push(AbiParam::new(types::I64));
        let fid = module.declare_function(ext, Linkage::Import, &sig)?;
        if let Some(int) = name_map.get(ext) { functions.insert(int.to_string(), fid); }
    }

    // main() entry point
    {
        let mut sig = module.make_signature();
        sig.returns.push(AbiParam::new(types::I32));
        let main_id = module.declare_function("main", Linkage::Export, &sig)?;
        ctx.func.signature = sig;
        let mut b = FunctionBuilder::new(&mut ctx.func, &mut builder_ctx);
        let blk = b.create_block(); b.switch_to_block(blk); b.seal_block(blk);
        if let Some(hid) = functions.get("головна") {
            let c = module.declare_func_in_func(*hid, b.func);
            b.ins().call(c, &[]);
        }
        let z = b.ins().iconst(types::I32, 0);
        b.ins().return_(&[z]);
        b.finalize();
        module.define_function(main_id, &mut ctx).map_err(|e| anyhow::anyhow!("main: {}", e))?;
        module.clear_context(&mut ctx);
    }

    for (name, params, body) in &func_decls {
        let fid = *functions.get(name).unwrap();
        ctx.func.signature = module.declarations().get_function_decl(fid).signature.clone();
        {
            let mut b = FunctionBuilder::new(&mut ctx.func, &mut builder_ctx);
            let entry = b.create_block();
            b.append_block_params_for_function_params(entry);
            b.switch_to_block(entry); b.seal_block(entry);
            let mut env = AotEnv::new();
            for (i, p) in params.iter().enumerate() {
                let v = b.block_params(entry)[i];
                let var = env.decl(p); b.declare_var(var, types::I64); b.def_var(var, v);
            }
            let fs = functions.clone();
            let mut t = AotTrans { b: &mut b, m: &mut module, e: &mut env, f: &fs, ret: false };
            t.body(body);
            if !t.ret { let z = t.b.ins().iconst(types::I64, 0); t.b.ins().return_(&[z]); }
            b.finalize();
        }
        module.define_function(fid, &mut ctx).map_err(|e| anyhow::anyhow!("{}: {}", name, e))?;
        module.clear_context(&mut ctx);
    }

    let product = module.finish();
    Ok(product.emit().map_err(|e| anyhow::anyhow!("{}", e))?)
}

pub fn compile_and_link(program: &Program, output: &str) -> anyhow::Result<()> {
    let obj = compile_to_object(program)?;
    let obj_path = format!("{}.o", output);
    let rt_path = format!("{}_rt.c", output);
    std::fs::write(&obj_path, &obj)?;
    std::fs::write(&rt_path, RT_C)?;

    let linked = ["gcc", "cc", "clang"].iter().any(|cc| {
        std::process::Command::new(cc)
            .args([&obj_path, &rt_path, "-o", output, "-lm"])
            .status().map(|s| s.success()).unwrap_or(false)
    });

    let _ = std::fs::remove_file(&obj_path);
    let _ = std::fs::remove_file(&rt_path);

    if linked { println!("Скомпільовано: {}", output); Ok(()) }
    else { Err(anyhow::anyhow!("Лінкер не знайдено (потрібен gcc/cc/clang)")) }
}

struct AotEnv { vars: HashMap<String, Variable>, n: usize }
impl AotEnv {
    fn new() -> Self { Self { vars: HashMap::new(), n: 0 } }
    fn decl(&mut self, name: &str) -> Variable {
        if let Some(v) = self.vars.get(name) { return *v; }
        let v = Variable::new(self.n); self.n += 1;
        self.vars.insert(name.to_string(), v); v
    }
    fn get(&self, name: &str) -> Option<Variable> { self.vars.get(name).copied() }
}

#[derive(Clone, Copy, PartialEq)]
enum Ty { I, F, S }

struct AotTrans<'a, 'b> {
    b: &'a mut FunctionBuilder<'b>,
    m: &'a mut ObjectModule,
    e: &'a mut AotEnv,
    f: &'a HashMap<String, FuncId>,
    ret: bool,
}

impl<'a, 'b> AotTrans<'a, 'b> {
    fn body(&mut self, stmts: &[Statement]) {
        for (i, s) in stmts.iter().enumerate() {
            if self.ret { return; }
            if i == stmts.len() - 1 { if let Statement::Expression(e) = s {
                let (v, _) = self.expr(e); self.b.ins().return_(&[v]); self.ret = true; return;
            }}
            self.stmt(s);
        }
    }

    fn stmt(&mut self, s: &Statement) {
        if self.ret { return; }
        match s {
            Statement::Return(Some(e)) => { let (v, _) = self.expr(e); self.b.ins().return_(&[v]); self.ret = true; }
            Statement::Return(None) => { let z = self.b.ins().iconst(types::I64, 0); self.b.ins().return_(&[z]); self.ret = true; }
            Statement::Expression(e) => { self.expr(e); }
            Statement::Declaration(Declaration::Variable { name, value, .. }) => {
                let (v, ty) = value.as_ref().map(|e| self.expr(e)).unwrap_or_else(|| (self.b.ins().iconst(types::I64, 0), Ty::I));
                let ct = if ty == Ty::F { types::F64 } else { types::I64 };
                let var = self.e.decl(name); self.b.declare_var(var, ct); self.b.def_var(var, v);
            }
            Statement::Assignment { target, value, op } => {
                if let Expression::Index { object, index } = target {
                    let a = self.expr(object).0; let i = self.expr(index).0; let v = self.expr(value).0;
                    self.rt("__array_set", &[a, i, v]);
                } else if let Expression::Identifier(name) = target {
                    if let Some(var) = self.e.get(name) {
                        let (nv, _) = self.expr(value);
                        let fv = match op {
                            AssignmentOp::Assign => nv,
                            AssignmentOp::AddAssign => { let o = self.b.use_var(var); self.b.ins().iadd(o, nv) }
                            AssignmentOp::SubAssign => { let o = self.b.use_var(var); self.b.ins().isub(o, nv) }
                            AssignmentOp::MulAssign => { let o = self.b.use_var(var); self.b.ins().imul(o, nv) }
                            AssignmentOp::DivAssign => { let o = self.b.use_var(var); self.b.ins().sdiv(o, nv) }
                            AssignmentOp::ModAssign => { let o = self.b.use_var(var); self.b.ins().srem(o, nv) }
                        };
                        self.b.def_var(var, fv);
                    }
                }
            }
            Statement::If { condition, then_branch, else_branch } => {
                let cv = self.expr(condition).0;
                let tb = self.b.create_block(); let eb = self.b.create_block(); let mb = self.b.create_block();
                let c = self.b.ins().icmp_imm(IntCC::NotEqual, cv, 0);
                self.b.ins().brif(c, tb, &[], eb, &[]);
                self.b.switch_to_block(tb); self.b.seal_block(tb);
                self.one_stmt(then_branch);
                if !self.ret { self.b.ins().jump(mb, &[]); } let tr = self.ret; self.ret = false;
                self.b.switch_to_block(eb); self.b.seal_block(eb);
                if let Some(es) = else_branch { self.one_stmt(es); }
                if !self.ret { self.b.ins().jump(mb, &[]); } let er = self.ret; self.ret = false;
                if tr && er { self.ret = true; } else { self.b.switch_to_block(mb); self.b.seal_block(mb); }
            }
            Statement::While { condition, body } => {
                let h = self.b.create_block(); let bb = self.b.create_block(); let ex = self.b.create_block();
                self.b.ins().jump(h, &[]); self.b.switch_to_block(h);
                let cv = self.expr(condition).0;
                let c = self.b.ins().icmp_imm(IntCC::NotEqual, cv, 0);
                self.b.ins().brif(c, bb, &[], ex, &[]);
                self.b.switch_to_block(bb); self.b.seal_block(bb);
                self.one_stmt(body);
                if !self.ret { self.b.ins().jump(h, &[]); } self.ret = false;
                self.b.seal_block(h); self.b.switch_to_block(ex); self.b.seal_block(ex);
            }
            Statement::For { variable, from, to, body, .. } => {
                let fv = self.expr(from).0; let var = self.e.decl(variable);
                self.b.declare_var(var, types::I64); self.b.def_var(var, fv);
                let h = self.b.create_block(); let bb = self.b.create_block(); let ex = self.b.create_block();
                self.b.ins().jump(h, &[]); self.b.switch_to_block(h);
                let tv = self.expr(to).0; let iv = self.b.use_var(var);
                let c = self.b.ins().icmp(IntCC::SignedLessThan, iv, tv);
                self.b.ins().brif(c, bb, &[], ex, &[]);
                self.b.switch_to_block(bb); self.b.seal_block(bb);
                self.one_stmt(body);
                let cur = self.b.use_var(var); let one = self.b.ins().iconst(types::I64, 1);
                let nxt = self.b.ins().iadd(cur, one); self.b.def_var(var, nxt);
                self.b.ins().jump(h, &[]); self.b.seal_block(h);
                self.b.switch_to_block(ex); self.b.seal_block(ex);
            }
            Statement::Block(ss) => self.body(ss),
            _ => {}
        }
    }

    fn one_stmt(&mut self, s: &Statement) {
        match s { Statement::Block(ss) => self.body(ss), _ => self.stmt(s) }
    }

    fn expr(&mut self, e: &Expression) -> (Value, Ty) {
        match e {
            Expression::Literal(Literal::Integer(n)) => (self.b.ins().iconst(types::I64, *n), Ty::I),
            Expression::Literal(Literal::Float(f)) => (self.b.ins().f64const(*f), Ty::F),
            Expression::Literal(Literal::Bool(v)) => (self.b.ins().iconst(types::I64, *v as i64), Ty::I),
            Expression::Literal(Literal::String(s)) => {
                let cs = std::ffi::CString::new(s.as_str()).unwrap_or_default();
                (self.b.ins().iconst(types::I64, cs.into_raw() as i64), Ty::S)
            }
            Expression::Identifier(n) => {
                if let Some(v) = self.e.get(n) { (self.b.use_var(v), Ty::I) }
                else { (self.b.ins().iconst(types::I64, 0), Ty::I) }
            }
            Expression::Binary { left, op, right } => {
                let (l, lt) = self.expr(left); let (r, rt) = self.expr(right);
                if (lt == Ty::S || rt == Ty::S) && matches!(op, BinaryOp::Add) {
                    return (self.rt("__concat", &[l, r]), Ty::S);
                }
                if lt == Ty::F || rt == Ty::F {
                    let fl = if lt == Ty::I { self.b.ins().fcvt_from_sint(types::F64, l) } else { l };
                    let fr = if rt == Ty::I { self.b.ins().fcvt_from_sint(types::F64, r) } else { r };
                    return match op {
                        BinaryOp::Add => (self.b.ins().fadd(fl, fr), Ty::F),
                        BinaryOp::Sub => (self.b.ins().fsub(fl, fr), Ty::F),
                        BinaryOp::Mul => (self.b.ins().fmul(fl, fr), Ty::F),
                        BinaryOp::Div => (self.b.ins().fdiv(fl, fr), Ty::F),
                        _ => { let c = self.b.ins().fcmp(FloatCC::Equal, fl, fr); (self.b.ins().uextend(types::I64, c), Ty::I) }
                    };
                }
                match op {
                    BinaryOp::Add => (self.b.ins().iadd(l, r), Ty::I),
                    BinaryOp::Sub => (self.b.ins().isub(l, r), Ty::I),
                    BinaryOp::Mul => (self.b.ins().imul(l, r), Ty::I),
                    BinaryOp::Div => (self.b.ins().sdiv(l, r), Ty::I),
                    BinaryOp::Mod => (self.b.ins().srem(l, r), Ty::I),
                    BinaryOp::Eq | BinaryOp::Ne | BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge => {
                        let cc = match op { BinaryOp::Eq => IntCC::Equal, BinaryOp::Ne => IntCC::NotEqual, BinaryOp::Lt => IntCC::SignedLessThan, BinaryOp::Le => IntCC::SignedLessThanOrEqual, BinaryOp::Gt => IntCC::SignedGreaterThan, _ => IntCC::SignedGreaterThanOrEqual };
                        let c = self.b.ins().icmp(cc, l, r); (self.b.ins().uextend(types::I64, c), Ty::I)
                    }
                    BinaryOp::And => (self.b.ins().band(l, r), Ty::I),
                    BinaryOp::Or => (self.b.ins().bor(l, r), Ty::I),
                    _ => (self.b.ins().iconst(types::I64, 0), Ty::I),
                }
            }
            Expression::Unary { op, operand } => {
                let (v, t) = self.expr(operand);
                match op {
                    UnaryOp::Neg => if t == Ty::F { (self.b.ins().fneg(v), Ty::F) } else { (self.b.ins().ineg(v), Ty::I) },
                    UnaryOp::Not => { let z = self.b.ins().iconst(types::I64, 0); let c = self.b.ins().icmp(IntCC::Equal, v, z); (self.b.ins().uextend(types::I64, c), Ty::I) }
                    _ => (v, t),
                }
            }
            Expression::Call { callee, args } => {
                if let Expression::Identifier(name) = callee.as_ref() {
                    if name == "друк" && args.len() == 1 {
                        let (v, t) = self.expr(&args[0]);
                        match t {
                            Ty::F => { self.rt("__друк_дрб", &[v]); }
                            Ty::S => { let z = self.b.ins().iconst(types::I64, 0); self.rt("__друк_рядок", &[v, z]); }
                            _ => { self.rt("друк", &[v]); }
                        }
                        return (self.b.ins().iconst(types::I64, 0), Ty::I);
                    }
                    if let Some(fid) = self.f.get(name.as_str()) {
                        let c = self.m.declare_func_in_func(*fid, self.b.func);
                        let av: Vec<Value> = args.iter().map(|a| self.expr(a).0).collect();
                        let call = self.b.ins().call(c, &av);
                        return (self.b.inst_results(call)[0], Ty::I);
                    }
                }
                (self.b.ins().iconst(types::I64, 0), Ty::I)
            }
            Expression::Array(elems) => {
                let cap = self.b.ins().iconst(types::I64, elems.len() as i64);
                let arr = self.rt("__array_new", &[cap]);
                for el in elems { let v = self.expr(el).0; self.rt("__array_push", &[arr, v]); }
                (arr, Ty::I)
            }
            Expression::Index { object, index } => {
                let a = self.expr(object).0; let i = self.expr(index).0;
                (self.rt("__array_get", &[a, i]), Ty::I)
            }
            Expression::FormatString(parts) => {
                let empty = std::ffi::CString::new("").unwrap_or_default();
                let mut r = self.b.ins().iconst(types::I64, empty.into_raw() as i64);
                for p in parts {
                    let pv = match p {
                        FormatPart::Text(s) => { let cs = std::ffi::CString::new(s.as_str()).unwrap_or_default(); self.b.ins().iconst(types::I64, cs.into_raw() as i64) }
                        FormatPart::Expr(ex) => { let (v, t) = self.expr(ex); match t { Ty::F => self.rt("__format_f64", &[v]), Ty::S => v, _ => self.rt("__format_int", &[v]) } }
                    };
                    r = self.rt("__concat", &[r, pv]);
                }
                (r, Ty::S)
            }
            _ => (self.b.ins().iconst(types::I64, 0), Ty::I),
        }
    }

    fn rt(&mut self, name: &str, args: &[Value]) -> Value {
        if let Some(fid) = self.f.get(name) {
            let c = self.m.declare_func_in_func(*fid, self.b.func);
            let call = self.b.ins().call(c, args);
            self.b.inst_results(call)[0]
        } else { self.b.ins().iconst(types::I64, 0) }
    }
}

const RT_C: &str = r#"#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <math.h>
typedef long long i64;
typedef double f64;
typedef struct { i64* data; i64 len; i64 cap; } TArr;
i64 __tryzub_print(i64 v) { printf("%lld\n", v); return 0; }
i64 __tryzub_print_f64(f64 v) { if(v==floor(v)&&isfinite(v)) printf("%.1f\n",v); else printf("%g\n",v); return 0; }
i64 __tryzub_print_str(i64 p, i64 l) { if(p) printf("%s\n",(char*)p); return 0; }
i64 __tryzub_concat(i64 a, i64 b) { const char*sa=a?(const char*)a:""; const char*sb=b?(const char*)b:""; i64 la=strlen(sa),lb=strlen(sb); char*r=(char*)malloc(la+lb+1); memcpy(r,sa,la); memcpy(r+la,sb,lb); r[la+lb]=0; return(i64)r; }
i64 __tryzub_array_new(i64 c) { TArr*a=(TArr*)malloc(sizeof(TArr)); a->data=(i64*)calloc(c>0?c:4,sizeof(i64)); a->len=0; a->cap=c>0?c:4; return(i64)a; }
i64 __tryzub_array_push(i64 ap, i64 v) { if(!ap)return 0; TArr*a=(TArr*)ap; if(a->len>=a->cap){a->cap*=2;a->data=(i64*)realloc(a->data,a->cap*sizeof(i64));} a->data[a->len++]=v; return ap; }
i64 __tryzub_array_get(i64 ap, i64 i) { if(!ap)return 0; TArr*a=(TArr*)ap; if(i<0||i>=a->len)return 0; return a->data[i]; }
i64 __tryzub_array_set(i64 ap, i64 i, i64 v) { if(!ap)return 0; TArr*a=(TArr*)ap; if(i>=0&&i<a->len)a->data[i]=v; return 0; }
i64 __tryzub_array_len(i64 ap) { if(!ap)return 0; return((TArr*)ap)->len; }
i64 __tryzub_format_int(i64 v) { char*b=(char*)malloc(32); snprintf(b,32,"%lld",v); return(i64)b; }
i64 __tryzub_format_f64(f64 v) { char*b=(char*)malloc(64); if(v==floor(v)&&isfinite(v))snprintf(b,64,"%.1f",v); else snprintf(b,64,"%g",v); return(i64)b; }
"#;

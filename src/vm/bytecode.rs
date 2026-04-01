// Тризуб Bytecode VM — стековий автомат

#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(u8)]
pub enum Op {
    Const, Pop, Dup,
    LoadLocal, StoreLocal, LoadGlobal, StoreGlobal,
    Add, Sub, Mul, Div, Mod, Pow, Neg,
    Eq, Ne, Lt, Le, Gt, Ge,
    Not, And, Or,
    BitAnd, BitOr, BitXor, Shl, Shr, BitNot,
    Jump, JumpIfFalse, JumpIfTrue, Loop,
    Call, Return,
    Print, Halt,
    Inc, AddAssign,
}

#[derive(Debug, Clone)]
pub enum BcValue {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(Box<str>),
    Null,
}

impl BcValue {
    #[inline(always)]
    pub fn as_int(&self) -> i64 {
        match self { BcValue::Int(n) => *n, BcValue::Float(f) => *f as i64, BcValue::Bool(b) => *b as i64, _ => 0 }
    }
    #[inline(always)]
    pub fn as_float(&self) -> f64 {
        match self { BcValue::Float(f) => *f, BcValue::Int(n) => *n as f64, _ => 0.0 }
    }
    #[inline(always)]
    pub fn as_bool(&self) -> bool {
        match self { BcValue::Bool(b) => *b, BcValue::Int(n) => *n != 0, BcValue::Float(f) => *f != 0.0, BcValue::Null => false, BcValue::Str(s) => !s.is_empty() }
    }
    pub fn to_string(&self) -> String {
        match self {
            BcValue::Int(n) => n.to_string(),
            BcValue::Float(f) => if *f == f.floor() && f.is_finite() { format!("{:.1}", f) } else { f.to_string() },
            BcValue::Bool(b) => if *b { "істина" } else { "хиба" }.into(),
            BcValue::Str(s) => s.to_string(),
            BcValue::Null => "нуль".into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Instruction {
    pub op: Op,
    pub arg: u32,
}

#[derive(Debug, Clone)]
pub struct Chunk {
    pub code: Vec<Instruction>,
    pub constants: Vec<BcValue>,
    pub local_count: usize,
    pub arg_count: usize,
    pub func_starts: std::collections::HashMap<String, usize>,
}

impl Default for Chunk {
    fn default() -> Self { Self::new() }
}

impl Chunk {
    pub fn new() -> Self {
        Self { code: Vec::new(), constants: Vec::new(), local_count: 0, arg_count: 0, func_starts: std::collections::HashMap::new() }
    }
    pub fn emit(&mut self, op: Op, arg: u32) -> usize {
        let idx = self.code.len();
        self.code.push(Instruction { op, arg });
        idx
    }
    pub fn add_constant(&mut self, val: BcValue) -> u32 {
        self.constants.push(val);
        (self.constants.len() - 1) as u32
    }
    pub fn patch_jump(&mut self, idx: usize, target: u32) {
        self.code[idx].arg = target;
    }
}

struct CallFrame {
    return_ip: usize,
    chunk_idx: usize,
    base_slot: usize,
}

pub struct BytecodeVM {
    stack: Vec<BcValue>,
    locals: Vec<BcValue>,
    globals: Vec<BcValue>,
    pub ops_executed: u64,
    pub functions: Vec<Chunk>,
}

impl BytecodeVM {
    pub fn new(local_count: usize) -> Self {
        let mut locals = Vec::with_capacity(local_count.max(1024));
        locals.resize(local_count.max(1024), BcValue::Null);
        Self {
            stack: Vec::with_capacity(512),
            locals,
            globals: Vec::new(),
            ops_executed: 0,
            functions: Vec::new(),
        }
    }

    pub fn register_function(&mut self, chunk: Chunk) -> usize {
        let idx = self.functions.len();
        self.functions.push(chunk);
        idx
    }

    #[inline(always)]
    fn push(&mut self, val: BcValue) { self.stack.push(val); }
    #[inline(always)]
    fn pop(&mut self) -> BcValue { self.stack.pop().unwrap_or(BcValue::Null) }

    pub fn execute(&mut self, main_chunk: &Chunk) -> BcValue {
        let main_idx = self.functions.len();
        self.functions.push(main_chunk.clone());

        let mut ip: usize = 0;
        let mut chunk_idx: usize = main_idx;
        let mut base_slot: usize = 0;
        let mut call_stack: Vec<CallFrame> = Vec::with_capacity(256);

        loop {
            let code = &self.functions[chunk_idx].code;
            if ip >= code.len() { break; }

            self.ops_executed += 1;
            let op = code[ip].op;
            let arg = code[ip].arg;
            ip += 1;

            match op {
                Op::Const => {
                    self.push(self.functions[chunk_idx].constants[arg as usize].clone());
                }
                Op::Pop => { self.pop(); }
                Op::Dup => {
                    let v = self.stack.last().cloned().unwrap_or(BcValue::Null);
                    self.push(v);
                }

                Op::LoadLocal => {
                    let idx = base_slot + arg as usize;
                    self.push(self.locals[idx].clone());
                }
                Op::StoreLocal => {
                    let val = self.pop();
                    self.locals[base_slot + arg as usize] = val;
                }
                Op::LoadGlobal => {
                    self.push(self.globals.get(arg as usize).cloned().unwrap_or(BcValue::Null));
                }
                Op::StoreGlobal => {
                    let val = self.pop();
                    let i = arg as usize;
                    if i >= self.globals.len() { self.globals.resize(i + 1, BcValue::Null); }
                    self.globals[i] = val;
                }

                Op::Add => {
                    let b = self.pop(); let a = self.pop();
                    match (&a, &b) {
                        (BcValue::Int(x), BcValue::Int(y)) => self.push(BcValue::Int(x.wrapping_add(*y))),
                        (BcValue::Float(x), BcValue::Float(y)) => self.push(BcValue::Float(x + y)),
                        (BcValue::Str(x), BcValue::Str(y)) => { let mut s = x.to_string(); s.push_str(y); self.push(BcValue::Str(s.into_boxed_str())); }
                        _ => self.push(BcValue::Int(a.as_int() + b.as_int())),
                    }
                }
                Op::Sub => {
                    let b = self.pop(); let a = self.pop();
                    match (&a, &b) {
                        (BcValue::Int(x), BcValue::Int(y)) => self.push(BcValue::Int(x.wrapping_sub(*y))),
                        _ => self.push(BcValue::Float(a.as_float() - b.as_float())),
                    }
                }
                Op::Mul => {
                    let b = self.pop(); let a = self.pop();
                    match (&a, &b) {
                        (BcValue::Int(x), BcValue::Int(y)) => self.push(BcValue::Int(x.wrapping_mul(*y))),
                        _ => self.push(BcValue::Float(a.as_float() * b.as_float())),
                    }
                }
                Op::Div => {
                    let b = self.pop(); let a = self.pop();
                    let bv = b.as_float();
                    if bv == 0.0 { self.push(BcValue::Float(f64::NAN)); }
                    else { self.push(BcValue::Float(a.as_float() / bv)); }
                }
                Op::Mod => {
                    let b = self.pop(); let a = self.pop();
                    match (&a, &b) {
                        (BcValue::Int(x), BcValue::Int(y)) if *y != 0 => self.push(BcValue::Int(x % y)),
                        _ => self.push(BcValue::Float(a.as_float() % b.as_float())),
                    }
                }
                Op::Pow => {
                    let b = self.pop(); let a = self.pop();
                    match (&a, &b) {
                        (BcValue::Int(x), BcValue::Int(y)) if *y >= 0 => self.push(BcValue::Int(x.pow(*y as u32))),
                        _ => self.push(BcValue::Float(a.as_float().powf(b.as_float()))),
                    }
                }
                Op::Neg => {
                    let a = self.pop();
                    match a { BcValue::Int(n) => self.push(BcValue::Int(-n)), BcValue::Float(f) => self.push(BcValue::Float(-f)), _ => self.push(BcValue::Int(0)) }
                }

                Op::Eq => { let b = self.pop(); let a = self.pop(); self.push(BcValue::Bool(a.as_int() == b.as_int())); }
                Op::Ne => { let b = self.pop(); let a = self.pop(); self.push(BcValue::Bool(a.as_int() != b.as_int())); }
                Op::Lt => { let b = self.pop(); let a = self.pop(); self.push(BcValue::Bool(a.as_int() < b.as_int())); }
                Op::Le => { let b = self.pop(); let a = self.pop(); self.push(BcValue::Bool(a.as_int() <= b.as_int())); }
                Op::Gt => { let b = self.pop(); let a = self.pop(); self.push(BcValue::Bool(a.as_int() > b.as_int())); }
                Op::Ge => { let b = self.pop(); let a = self.pop(); self.push(BcValue::Bool(a.as_int() >= b.as_int())); }

                Op::Not => { let a = self.pop(); self.push(BcValue::Bool(!a.as_bool())); }
                Op::And => { let b = self.pop(); let a = self.pop(); self.push(BcValue::Bool(a.as_bool() && b.as_bool())); }
                Op::Or => { let b = self.pop(); let a = self.pop(); self.push(BcValue::Bool(a.as_bool() || b.as_bool())); }

                Op::BitAnd => { let b = self.pop(); let a = self.pop(); self.push(BcValue::Int(a.as_int() & b.as_int())); }
                Op::BitOr => { let b = self.pop(); let a = self.pop(); self.push(BcValue::Int(a.as_int() | b.as_int())); }
                Op::BitXor => { let b = self.pop(); let a = self.pop(); self.push(BcValue::Int(a.as_int() ^ b.as_int())); }
                Op::Shl => { let b = self.pop(); let a = self.pop(); self.push(BcValue::Int(a.as_int() << b.as_int())); }
                Op::Shr => { let b = self.pop(); let a = self.pop(); self.push(BcValue::Int(a.as_int() >> b.as_int())); }
                Op::BitNot => { let a = self.pop(); self.push(BcValue::Int(!a.as_int())); }

                Op::Jump => { ip = arg as usize; }
                Op::JumpIfFalse => { let c = self.pop(); if !c.as_bool() { ip = arg as usize; } }
                Op::JumpIfTrue => { let c = self.pop(); if c.as_bool() { ip = arg as usize; } }
                Op::Loop => { ip -= arg as usize; }

                Op::Call => {
                    let func_idx = arg as usize;
                    if func_idx >= self.functions.len() { continue; }
                    let ac = self.functions[func_idx].arg_count;
                    let lc = self.functions[func_idx].local_count;
                    let new_base = self.locals.len();
                    if new_base + lc.max(ac).max(16) >= self.locals.len() {
                        self.locals.resize(new_base + lc.max(ac).max(16) + 16, BcValue::Null);
                    }
                    for i in (0..ac).rev() {
                        self.locals[new_base + i] = self.pop();
                    }
                    call_stack.push(CallFrame { return_ip: ip, chunk_idx, base_slot });
                    ip = 0;
                    chunk_idx = func_idx;
                    base_slot = new_base;
                }
                Op::Return => {
                    let result = self.pop();
                    if let Some(frame) = call_stack.pop() {
                        self.locals.truncate(base_slot);
                        ip = frame.return_ip;
                        chunk_idx = frame.chunk_idx;
                        base_slot = frame.base_slot;
                        self.push(result);
                    } else {
                        self.push(result);
                        break;
                    }
                }

                Op::Print => { let val = self.pop(); println!("{}", val.to_string()); }
                Op::Halt => { break; }

                Op::Inc => {
                    let idx = base_slot + arg as usize;
                    if let BcValue::Int(n) = &self.locals[idx] { self.locals[idx] = BcValue::Int(n + 1); }
                }
                Op::AddAssign => {
                    let val = self.pop();
                    let idx = base_slot + arg as usize;
                    match (&self.locals[idx], &val) {
                        (BcValue::Int(a), BcValue::Int(b)) => self.locals[idx] = BcValue::Int(a.wrapping_add(*b)),
                        _ => { let sum = self.locals[idx].as_float() + val.as_float(); self.locals[idx] = BcValue::Float(sum); }
                    }
                }
            }
        }

        self.pop()
    }
}

pub fn benchmark_sum_bytecode(n: i64) -> (i64, std::time::Duration) {
    let mut chunk = Chunk::new();
    chunk.local_count = 2;
    let zero = chunk.add_constant(BcValue::Int(0));
    chunk.emit(Op::Const, zero);
    chunk.emit(Op::StoreLocal, 0);
    let one = chunk.add_constant(BcValue::Int(1));
    chunk.emit(Op::Const, one);
    chunk.emit(Op::StoreLocal, 1);
    let loop_start = chunk.code.len();
    chunk.emit(Op::LoadLocal, 1);
    let n_const = chunk.add_constant(BcValue::Int(n));
    chunk.emit(Op::Const, n_const);
    chunk.emit(Op::Ge, 0);
    let jump_end = chunk.emit(Op::JumpIfTrue, 0);
    chunk.emit(Op::LoadLocal, 1);
    chunk.emit(Op::AddAssign, 0);
    chunk.emit(Op::Inc, 1);
    let loop_body_size = (chunk.code.len() - loop_start + 1) as u32;
    chunk.emit(Op::Loop, loop_body_size);
    let end = chunk.code.len();
    chunk.patch_jump(jump_end, end as u32);
    chunk.emit(Op::LoadLocal, 0);
    chunk.emit(Op::Halt, 0);
    let mut vm = BytecodeVM::new(chunk.local_count);
    let start = std::time::Instant::now();
    let result = vm.execute(&chunk);
    let elapsed = start.elapsed();
    (result.as_int(), elapsed)
}

pub fn benchmark_fibonacci_bytecode(n: i64) -> (i64, std::time::Duration) {
    // fib(n): if n <= 1 return n; return fib(n-1) + fib(n-2)
    let mut fib_chunk = Chunk::new();
    fib_chunk.local_count = 1;
    fib_chunk.arg_count = 1;
    let c1 = fib_chunk.add_constant(BcValue::Int(1));
    let c2 = fib_chunk.add_constant(BcValue::Int(2));
    // if n <= 1
    fib_chunk.emit(Op::LoadLocal, 0); // n
    fib_chunk.emit(Op::Const, c1);    // 1
    fib_chunk.emit(Op::Le, 0);
    let jmp = fib_chunk.emit(Op::JumpIfFalse, 0);
    fib_chunk.emit(Op::LoadLocal, 0);
    fib_chunk.emit(Op::Return, 0);
    let after = fib_chunk.code.len();
    fib_chunk.patch_jump(jmp, after as u32);
    // fib(n-1)
    fib_chunk.emit(Op::LoadLocal, 0);
    fib_chunk.emit(Op::Const, c1);
    fib_chunk.emit(Op::Sub, 0);
    fib_chunk.emit(Op::Call, 0); // call fib (func_idx=0)
    // fib(n-2)
    fib_chunk.emit(Op::LoadLocal, 0);
    fib_chunk.emit(Op::Const, c2);
    fib_chunk.emit(Op::Sub, 0);
    fib_chunk.emit(Op::Call, 0); // call fib (func_idx=0)
    // return fib(n-1) + fib(n-2)
    fib_chunk.emit(Op::Add, 0);
    fib_chunk.emit(Op::Return, 0);

    // main chunk
    let mut main_chunk = Chunk::new();
    main_chunk.local_count = 0;
    let n_const = main_chunk.add_constant(BcValue::Int(n));
    main_chunk.emit(Op::Const, n_const);
    main_chunk.emit(Op::Call, 0); // call fib
    main_chunk.emit(Op::Halt, 0);

    let mut vm = BytecodeVM::new(0);
    vm.register_function(fib_chunk); // idx=0

    let start = std::time::Instant::now();
    let result = vm.execute(&main_chunk);
    let elapsed = start.elapsed();
    (result.as_int(), elapsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bytecode_sum() {
        let (result, _) = benchmark_sum_bytecode(10001);
        assert_eq!(result, 50005000);
    }

    #[test]
    fn test_bytecode_arithmetic() {
        let mut chunk = Chunk::new();
        let a = chunk.add_constant(BcValue::Int(40));
        let b = chunk.add_constant(BcValue::Int(2));
        chunk.emit(Op::Const, a);
        chunk.emit(Op::Const, b);
        chunk.emit(Op::Add, 0);
        chunk.emit(Op::Halt, 0);
        let mut vm = BytecodeVM::new(0);
        let result = vm.execute(&chunk);
        assert_eq!(result.as_int(), 42);
    }

    #[test]
    fn test_bytecode_comparison() {
        let mut chunk = Chunk::new();
        let a = chunk.add_constant(BcValue::Int(5));
        let b = chunk.add_constant(BcValue::Int(3));
        chunk.emit(Op::Const, a);
        chunk.emit(Op::Const, b);
        chunk.emit(Op::Gt, 0);
        chunk.emit(Op::Halt, 0);
        let mut vm = BytecodeVM::new(0);
        let result = vm.execute(&chunk);
        assert!(result.as_bool());
    }

    #[test]
    fn test_bytecode_fibonacci() {
        let (result, elapsed) = benchmark_fibonacci_bytecode(25);
        assert_eq!(result, 75025);
        eprintln!("Fibonacci(25) bytecode: {}мс", elapsed.as_millis());
    }
}

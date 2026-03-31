// Тризуб Bytecode VM — стековий автомат для швидкого виконання

#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(u8)]
pub enum Op {
    // Стек
    Const,        // push constants[arg]
    Pop,          // pop top
    Dup,          // duplicate top

    // Змінні
    LoadLocal,    // push locals[arg]
    StoreLocal,   // locals[arg] = pop
    LoadGlobal,   // push globals[arg]
    StoreGlobal,  // globals[arg] = pop

    // Арифметика (pop 2, push 1)
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    Neg,          // unary minus

    // Порівняння
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,

    // Логіка
    Not,
    And,
    Or,

    // Побітові
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    BitNot,

    // Керування
    Jump,         // ip = arg
    JumpIfFalse,  // if !pop: ip = arg
    JumpIfTrue,   // if pop: ip = arg
    Loop,         // ip -= arg (backward jump)

    // Функції
    Call,         // call function with arg arguments
    Return,       // return top of stack

    // Вбудовані
    Print,        // print pop
    Halt,         // stop execution

    // Інкремент/декремент для циклів
    Inc,          // locals[arg] += 1
    AddAssign,    // locals[arg] += pop
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

#[derive(Debug)]
pub struct Chunk {
    pub code: Vec<Instruction>,
    pub constants: Vec<BcValue>,
    pub local_count: usize,
}

impl Default for Chunk {
    fn default() -> Self {
        Self::new()
    }
}

impl Chunk {
    pub fn new() -> Self {
        Self { code: Vec::new(), constants: Vec::new(), local_count: 0 }
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
    base_slot: usize,
    func_idx: usize,
}

pub struct BytecodeVM {
    stack: Vec<BcValue>,
    locals: Vec<BcValue>,
    globals: Vec<BcValue>,
    ip: usize,
    pub ops_executed: u64,
    call_stack: Vec<CallFrame>,
    functions: Vec<Chunk>,
}

impl BytecodeVM {
    pub fn new(local_count: usize) -> Self {
        let mut locals = Vec::with_capacity(local_count.max(256));
        locals.resize(local_count.max(256), BcValue::Null);
        Self {
            stack: Vec::with_capacity(256),
            locals,
            globals: Vec::new(),
            ip: 0,
            ops_executed: 0,
            call_stack: Vec::with_capacity(64),
            functions: Vec::new(),
        }
    }

    pub fn register_function(&mut self, chunk: Chunk) -> usize {
        let idx = self.functions.len();
        self.functions.push(chunk);
        idx
    }

    #[inline(always)]
    fn push(&mut self, val: BcValue) {
        self.stack.push(val);
    }

    #[inline(always)]
    fn pop(&mut self) -> BcValue {
        self.stack.pop().unwrap_or(BcValue::Null)
    }

    #[inline(always)]
    fn peek(&self) -> &BcValue {
        self.stack.last().unwrap_or(&BcValue::Null)
    }

    pub fn execute(&mut self, chunk: &Chunk) -> BcValue {
        self.ip = 0;
        let code = &chunk.code;
        let constants = &chunk.constants;
        let len = code.len();

        while self.ip < len {
            self.ops_executed += 1;
            let inst = &code[self.ip];
            self.ip += 1;

            match inst.op {
                Op::Const => {
                    self.push(constants[inst.arg as usize].clone());
                }
                Op::Pop => { self.pop(); }
                Op::Dup => {
                    let v = self.peek().clone();
                    self.push(v);
                }

                // Змінні
                Op::LoadLocal => {
                    self.push(self.locals[inst.arg as usize].clone());
                }
                Op::StoreLocal => {
                    let val = self.pop();
                    self.locals[inst.arg as usize] = val;
                }
                Op::LoadGlobal => {
                    self.push(self.globals.get(inst.arg as usize).cloned().unwrap_or(BcValue::Null));
                }
                Op::StoreGlobal => {
                    let val = self.pop();
                    let idx = inst.arg as usize;
                    if idx >= self.globals.len() { self.globals.resize(idx + 1, BcValue::Null); }
                    self.globals[idx] = val;
                }

                // Арифметика — оптимізована для int fast-path
                Op::Add => {
                    let b = self.pop();
                    let a = self.pop();
                    match (&a, &b) {
                        (BcValue::Int(x), BcValue::Int(y)) => self.push(BcValue::Int(x.wrapping_add(*y))),
                        (BcValue::Float(x), BcValue::Float(y)) => self.push(BcValue::Float(x + y)),
                        (BcValue::Int(x), BcValue::Float(y)) => self.push(BcValue::Float(*x as f64 + y)),
                        (BcValue::Float(x), BcValue::Int(y)) => self.push(BcValue::Float(x + *y as f64)),
                        (BcValue::Str(x), BcValue::Str(y)) => {
                            let mut s = x.to_string();
                            s.push_str(y);
                            self.push(BcValue::Str(s.into_boxed_str()));
                        }
                        _ => self.push(BcValue::Int(a.as_int() + b.as_int())),
                    }
                }
                Op::Sub => {
                    let b = self.pop();
                    let a = self.pop();
                    match (&a, &b) {
                        (BcValue::Int(x), BcValue::Int(y)) => self.push(BcValue::Int(x.wrapping_sub(*y))),
                        _ => self.push(BcValue::Float(a.as_float() - b.as_float())),
                    }
                }
                Op::Mul => {
                    let b = self.pop();
                    let a = self.pop();
                    match (&a, &b) {
                        (BcValue::Int(x), BcValue::Int(y)) => self.push(BcValue::Int(x.wrapping_mul(*y))),
                        _ => self.push(BcValue::Float(a.as_float() * b.as_float())),
                    }
                }
                Op::Div => {
                    let b = self.pop();
                    let a = self.pop();
                    let bv = b.as_float();
                    if bv == 0.0 { self.push(BcValue::Float(f64::NAN)); }
                    else { self.push(BcValue::Float(a.as_float() / bv)); }
                }
                Op::Mod => {
                    let b = self.pop();
                    let a = self.pop();
                    match (&a, &b) {
                        (BcValue::Int(x), BcValue::Int(y)) if *y != 0 => self.push(BcValue::Int(x % y)),
                        _ => self.push(BcValue::Float(a.as_float() % b.as_float())),
                    }
                }
                Op::Pow => {
                    let b = self.pop();
                    let a = self.pop();
                    match (&a, &b) {
                        (BcValue::Int(x), BcValue::Int(y)) if *y >= 0 => self.push(BcValue::Int(x.pow(*y as u32))),
                        _ => self.push(BcValue::Float(a.as_float().powf(b.as_float()))),
                    }
                }
                Op::Neg => {
                    let a = self.pop();
                    match a {
                        BcValue::Int(n) => self.push(BcValue::Int(-n)),
                        BcValue::Float(f) => self.push(BcValue::Float(-f)),
                        _ => self.push(BcValue::Int(0)),
                    }
                }

                // Порівняння
                Op::Eq => { let b = self.pop(); let a = self.pop(); self.push(BcValue::Bool(a.as_int() == b.as_int())); }
                Op::Ne => { let b = self.pop(); let a = self.pop(); self.push(BcValue::Bool(a.as_int() != b.as_int())); }
                Op::Lt => { let b = self.pop(); let a = self.pop(); self.push(BcValue::Bool(a.as_int() < b.as_int())); }
                Op::Le => { let b = self.pop(); let a = self.pop(); self.push(BcValue::Bool(a.as_int() <= b.as_int())); }
                Op::Gt => { let b = self.pop(); let a = self.pop(); self.push(BcValue::Bool(a.as_int() > b.as_int())); }
                Op::Ge => { let b = self.pop(); let a = self.pop(); self.push(BcValue::Bool(a.as_int() >= b.as_int())); }

                // Логіка
                Op::Not => { let a = self.pop(); self.push(BcValue::Bool(!a.as_bool())); }
                Op::And => { let b = self.pop(); let a = self.pop(); self.push(BcValue::Bool(a.as_bool() && b.as_bool())); }
                Op::Or => { let b = self.pop(); let a = self.pop(); self.push(BcValue::Bool(a.as_bool() || b.as_bool())); }

                // Побітові
                Op::BitAnd => { let b = self.pop(); let a = self.pop(); self.push(BcValue::Int(a.as_int() & b.as_int())); }
                Op::BitOr => { let b = self.pop(); let a = self.pop(); self.push(BcValue::Int(a.as_int() | b.as_int())); }
                Op::BitXor => { let b = self.pop(); let a = self.pop(); self.push(BcValue::Int(a.as_int() ^ b.as_int())); }
                Op::Shl => { let b = self.pop(); let a = self.pop(); self.push(BcValue::Int(a.as_int() << b.as_int())); }
                Op::Shr => { let b = self.pop(); let a = self.pop(); self.push(BcValue::Int(a.as_int() >> b.as_int())); }
                Op::BitNot => { let a = self.pop(); self.push(BcValue::Int(!a.as_int())); }

                // Керування потоком
                Op::Jump => { self.ip = inst.arg as usize; }
                Op::JumpIfFalse => {
                    let cond = self.pop();
                    if !cond.as_bool() { self.ip = inst.arg as usize; }
                }
                Op::JumpIfTrue => {
                    let cond = self.pop();
                    if cond.as_bool() { self.ip = inst.arg as usize; }
                }
                Op::Loop => { self.ip -= inst.arg as usize; }

                Op::Call => {
                    let func_idx = inst.arg as usize;
                    let arg_count = if func_idx < self.functions.len() {
                        self.functions[func_idx].local_count
                    } else { 0 };
                    self.call_stack.push(CallFrame {
                        return_ip: self.ip,
                        base_slot: self.locals.len(),
                        func_idx,
                    });
                    let base = self.locals.len();
                    self.locals.resize(base + arg_count.max(16), BcValue::Null);
                    let stack_args = std::cmp::min(self.stack.len(), arg_count);
                    for i in (0..stack_args).rev() {
                        self.locals[base + i] = self.stack.pop().unwrap_or(BcValue::Null);
                    }
                    if func_idx < self.functions.len() {
                        let func_code = self.functions[func_idx].code.clone();
                        let func_constants = self.functions[func_idx].constants.clone();
                        let saved_ip = self.ip;
                        self.ip = 0;
                        while self.ip < func_code.len() {
                            self.ops_executed += 1;
                            let fi = &func_code[self.ip];
                            self.ip += 1;
                            match fi.op {
                                Op::Const => self.push(func_constants[fi.arg as usize].clone()),
                                Op::LoadLocal => self.push(self.locals[base + fi.arg as usize].clone()),
                                Op::StoreLocal => { let v = self.pop(); self.locals[base + fi.arg as usize] = v; }
                                Op::Add => { let b = self.pop(); let a = self.pop(); match (&a,&b) { (BcValue::Int(x),BcValue::Int(y)) => self.push(BcValue::Int(x+y)), _ => self.push(BcValue::Float(a.as_float()+b.as_float())) } }
                                Op::Sub => { let b = self.pop(); let a = self.pop(); match (&a,&b) { (BcValue::Int(x),BcValue::Int(y)) => self.push(BcValue::Int(x-y)), _ => self.push(BcValue::Float(a.as_float()-b.as_float())) } }
                                Op::Mul => { let b = self.pop(); let a = self.pop(); match (&a,&b) { (BcValue::Int(x),BcValue::Int(y)) => self.push(BcValue::Int(x*y)), _ => self.push(BcValue::Float(a.as_float()*b.as_float())) } }
                                Op::Lt => { let b = self.pop(); let a = self.pop(); self.push(BcValue::Bool(a.as_int() < b.as_int())); }
                                Op::Le => { let b = self.pop(); let a = self.pop(); self.push(BcValue::Bool(a.as_int() <= b.as_int())); }
                                Op::Gt => { let b = self.pop(); let a = self.pop(); self.push(BcValue::Bool(a.as_int() > b.as_int())); }
                                Op::Eq => { let b = self.pop(); let a = self.pop(); self.push(BcValue::Bool(a.as_int() == b.as_int())); }
                                Op::JumpIfFalse => { let c = self.pop(); if !c.as_bool() { self.ip = fi.arg as usize; } }
                                Op::JumpIfTrue => { let c = self.pop(); if c.as_bool() { self.ip = fi.arg as usize; } }
                                Op::Jump => { self.ip = fi.arg as usize; }
                                Op::Call => {
                                    // Рекурсивний виклик
                                    let ridx = fi.arg as usize;
                                    let rargs = if ridx < self.functions.len() { self.functions[ridx].local_count } else { 0 };
                                    let rbase = self.locals.len();
                                    self.locals.resize(rbase + rargs.max(16), BcValue::Null);
                                    let sa = std::cmp::min(self.stack.len(), rargs);
                                    for i in (0..sa).rev() { self.locals[rbase + i] = self.stack.pop().unwrap_or(BcValue::Null); }
                                    self.call_stack.push(CallFrame { return_ip: self.ip, base_slot: base, func_idx: ridx });
                                    // Can't recurse properly inline — use iterative approach via stack
                                    // For now: fallback to the outer execute loop won't work
                                    // This needs a proper iterative call mechanism
                                }
                                Op::Return => break,
                                Op::Print => { let v = self.pop(); println!("{}", v.to_string()); }
                                _ => {}
                            }
                        }
                        self.ip = saved_ip;
                    }
                    self.locals.truncate(self.call_stack.pop().map_or(0, |f| f.base_slot));
                }
                Op::Return => {
                    if let Some(frame) = self.call_stack.pop() {
                        self.ip = frame.return_ip;
                        self.locals.truncate(frame.base_slot);
                    } else {
                        return self.pop();
                    }
                }

                // Вбудовані
                Op::Print => {
                    let val = self.pop();
                    println!("{}", val.to_string());
                }
                Op::Halt => { return self.pop(); }

                // Оптимізовані операції для циклів
                Op::Inc => {
                    let idx = inst.arg as usize;
                    if let BcValue::Int(n) = &self.locals[idx] {
                        self.locals[idx] = BcValue::Int(n + 1);
                    }
                }
                Op::AddAssign => {
                    let val = self.pop();
                    let idx = inst.arg as usize;
                    match (&self.locals[idx], &val) {
                        (BcValue::Int(a), BcValue::Int(b)) => {
                            self.locals[idx] = BcValue::Int(a.wrapping_add(*b));
                        }
                        _ => {
                            let sum = self.locals[idx].as_float() + val.as_float();
                            self.locals[idx] = BcValue::Float(sum);
                        }
                    }
                }
            }
        }

        self.pop()
    }
}

// Бенчмарк: сума 1..N через bytecode
pub fn benchmark_sum_bytecode(n: i64) -> (i64, std::time::Duration) {
    let mut chunk = Chunk::new();
    chunk.local_count = 2; // 0=сума, 1=і

    // сума = 0
    let zero = chunk.add_constant(BcValue::Int(0));
    chunk.emit(Op::Const, zero);
    chunk.emit(Op::StoreLocal, 0);

    // і = 1
    let one = chunk.add_constant(BcValue::Int(1));
    chunk.emit(Op::Const, one);
    chunk.emit(Op::StoreLocal, 1);

    // loop_start:
    let loop_start = chunk.code.len();

    // if і >= n: jump to end
    chunk.emit(Op::LoadLocal, 1);
    let n_const = chunk.add_constant(BcValue::Int(n));
    chunk.emit(Op::Const, n_const);
    chunk.emit(Op::Ge, 0);
    let jump_end = chunk.emit(Op::JumpIfTrue, 0); // patch later

    // сума += і
    chunk.emit(Op::LoadLocal, 1);
    chunk.emit(Op::AddAssign, 0);

    // і += 1
    chunk.emit(Op::Inc, 1);

    // jump loop_start
    let loop_body_size = (chunk.code.len() - loop_start + 1) as u32;
    chunk.emit(Op::Loop, loop_body_size);

    // end:
    let end = chunk.code.len();
    chunk.patch_jump(jump_end, end as u32);

    // return сума
    chunk.emit(Op::LoadLocal, 0);
    chunk.emit(Op::Halt, 0);

    let mut vm = BytecodeVM::new(chunk.local_count);
    let start = std::time::Instant::now();
    let result = vm.execute(&chunk);
    let elapsed = start.elapsed();

    (result.as_int(), elapsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bytecode_sum() {
        let (result, _) = benchmark_sum_bytecode(10001);
        assert_eq!(result, 50005000); // sum 1..10000
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
}

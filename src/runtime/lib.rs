use anyhow::Result;
use crossbeam_channel::{bounded, Receiver, Sender};
use dashmap::DashMap;
use once_cell::sync::Lazy;
use parking_lot::{Mutex, RwLock};
use std::alloc::{alloc, dealloc, Layout};
use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};
use std::ptr;
use std::sync::Arc;
use std::thread;
use thiserror::Error;

// ===== Система управління пам'яттю =====

#[derive(Debug)]
pub struct MemoryManager {
    allocations: DashMap<*mut u8, AllocationInfo>,
    total_allocated: Arc<Mutex<usize>>,
    allocation_limit: usize,
}

#[derive(Debug)]
struct AllocationInfo {
    size: usize,
    layout: Layout,
    source_location: Option<String>,
}

impl MemoryManager {
    pub fn new(limit: usize) -> Self {
        Self {
            allocations: DashMap::new(),
            total_allocated: Arc::new(Mutex::new(0)),
            allocation_limit: limit,
        }
    }
    
    pub unsafe fn allocate(&self, size: usize, align: usize, location: Option<String>) -> Result<*mut u8> {
        let layout = Layout::from_size_align(size, align)?;
        
        let mut total = self.total_allocated.lock();
        if *total + size > self.allocation_limit {
            return Err(MemoryError::OutOfMemory { requested: size, available: self.allocation_limit - *total }.into());
        }
        
        let ptr = alloc(layout);
        if ptr.is_null() {
            return Err(MemoryError::AllocationFailed { size }.into());
        }
        
        *total += size;
        self.allocations.insert(ptr, AllocationInfo { size, layout, source_location: location });
        
        Ok(ptr)
    }
    
    pub unsafe fn deallocate(&self, ptr: *mut u8) -> Result<()> {
        if let Some((_, info)) = self.allocations.remove(&ptr) {
            dealloc(ptr, info.layout);
            *self.total_allocated.lock() -= info.size;
            Ok(())
        } else {
            Err(MemoryError::InvalidPointer { ptr }.into())
        }
    }
    
    pub fn get_stats(&self) -> MemoryStats {
        MemoryStats {
            total_allocated: *self.total_allocated.lock(),
            allocation_count: self.allocations.len(),
            allocation_limit: self.allocation_limit,
        }
    }
}

#[derive(Debug)]
pub struct MemoryStats {
    pub total_allocated: usize,
    pub allocation_count: usize,
    pub allocation_limit: usize,
}

#[derive(Error, Debug)]
pub enum MemoryError {
    #[error("Недостатньо пам'яті: запитано {requested} байт, доступно {available} байт")]
    OutOfMemory { requested: usize, available: usize },
    
    #[error("Не вдалося виділити {size} байт пам'яті")]
    AllocationFailed { size: usize },
    
    #[error("Невалідний покажчик: {ptr:?}")]
    InvalidPointer { ptr: *mut u8 },
}

// Глобальний менеджер пам'яті
static MEMORY_MANAGER: Lazy<MemoryManager> = Lazy::new(|| {
    MemoryManager::new(1024 * 1024 * 1024) // 1GB ліміт
});

// ===== Система багатопоточності =====

pub struct ThreadPool {
    workers: Vec<Worker>,
    sender: Sender<Job>,
}

type Job = Box<dyn FnOnce() + Send + 'static>;

struct Worker {
    id: usize,
    thread: Option<thread::JoinHandle<()>>,
}

impl ThreadPool {
    pub fn new(size: usize) -> Self {
        let (sender, receiver) = bounded(size * 2);
        let receiver = Arc::new(Mutex::new(receiver));
        
        let mut workers = Vec::with_capacity(size);
        
        for id in 0..size {
            workers.push(Worker::new(id, Arc::clone(&receiver)));
        }
        
        ThreadPool { workers, sender }
    }
    
    pub fn execute<F>(&self, f: F) -> Result<()>
    where
        F: FnOnce() + Send + 'static,
    {
        let job = Box::new(f);
        self.sender.send(job)?;
        Ok(())
    }
    
    pub fn shutdown(&mut self) {
        drop(self.sender.clone());
        
        for worker in &mut self.workers {
            if let Some(thread) = worker.thread.take() {
                thread.join().unwrap();
            }
        }
    }
}

impl Worker {
    fn new(id: usize, receiver: Arc<Mutex<Receiver<Job>>>) -> Worker {
        let thread = thread::spawn(move || loop {
            let job = receiver.lock().recv();
            
            match job {
                Ok(job) => {
                    job();
                }
                Err(_) => {
                    break;
                }
            }
        });
        
        Worker {
            id,
            thread: Some(thread),
        }
    }
}

// ===== Система обробки помилок =====

#[derive(Debug, Clone)]
pub struct TryzubError {
    pub kind: ErrorKind,
    pub message: String,
    pub source_location: Option<SourceLocation>,
    pub stack_trace: Vec<StackFrame>,
}

#[derive(Debug, Clone)]
pub enum ErrorKind {
    TypeError,
    NameError,
    IndexError,
    ValueError,
    RuntimeError,
    MemoryError,
    ThreadError,
    IOError,
    SyntaxError,
    SystemError,
}

#[derive(Debug, Clone)]
pub struct SourceLocation {
    pub file: String,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone)]
pub struct StackFrame {
    pub function_name: String,
    pub location: SourceLocation,
}

impl TryzubError {
    pub fn new(kind: ErrorKind, message: String) -> Self {
        Self {
            kind,
            message,
            source_location: None,
            stack_trace: Vec::new(),
        }
    }
    
    pub fn with_location(mut self, location: SourceLocation) -> Self {
        self.source_location = Some(location);
        self
    }
    
    pub fn add_stack_frame(&mut self, frame: StackFrame) {
        self.stack_trace.push(frame);
    }
    
    pub fn format_error(&self) -> String {
        let mut output = format!("{:?}: {}", self.kind, self.message);
        
        if let Some(loc) = &self.source_location {
            output.push_str(&format!("\n  Файл: {}, рядок {}, колонка {}", loc.file, loc.line, loc.column));
        }
        
        if !self.stack_trace.is_empty() {
            output.push_str("\n\nСтек викликів:");
            for frame in &self.stack_trace {
                output.push_str(&format!(
                    "\n  {} у {}:{}:{}",
                    frame.function_name, frame.location.file, frame.location.line, frame.location.column
                ));
            }
        }
        
        output
    }
}

// ===== FFI (Foreign Function Interface) =====

#[repr(C)]
pub struct TryzubValue {
    pub value_type: ValueType,
    pub data: ValueData,
}

#[repr(C)]
pub enum ValueType {
    Null,
    Integer,
    Float,
    Boolean,
    String,
    Array,
    Object,
}

#[repr(C)]
pub union ValueData {
    pub null: (),
    pub integer: i64,
    pub float: f64,
    pub boolean: bool,
    pub string: *mut c_char,
    pub array: *mut TryzubArray,
    pub object: *mut TryzubObject,
}

#[repr(C)]
pub struct TryzubArray {
    pub length: usize,
    pub capacity: usize,
    pub elements: *mut TryzubValue,
}

#[repr(C)]
pub struct TryzubObject {
    pub fields: *mut c_void,
}

// FFI функції для взаємодії з C
#[no_mangle]
pub extern "C" fn tryzub_create_integer(value: i64) -> *mut TryzubValue {
    let val = Box::new(TryzubValue {
        value_type: ValueType::Integer,
        data: ValueData { integer: value },
    });
    Box::into_raw(val)
}

#[no_mangle]
pub extern "C" fn tryzub_create_float(value: f64) -> *mut TryzubValue {
    let val = Box::new(TryzubValue {
        value_type: ValueType::Float,
        data: ValueData { float: value },
    });
    Box::into_raw(val)
}

#[no_mangle]
pub extern "C" fn tryzub_create_string(s: *const c_char) -> *mut TryzubValue {
    unsafe {
        let c_str = CStr::from_ptr(s);
        let string = CString::new(c_str.to_bytes()).unwrap();
        let val = Box::new(TryzubValue {
            value_type: ValueType::String,
            data: ValueData { string: string.into_raw() },
        });
        Box::into_raw(val)
    }
}

#[no_mangle]
pub extern "C" fn tryzub_free_value(value: *mut TryzubValue) {
    unsafe {
        if !value.is_null() {
            let val = Box::from_raw(value);
            match val.value_type {
                ValueType::String => {
                    if !val.data.string.is_null() {
                        let _ = CString::from_raw(val.data.string);
                    }
                }
                ValueType::Array => {
                    if !val.data.array.is_null() {
                        let array = Box::from_raw(val.data.array);
                        // Звільняємо елементи масиву
                        for i in 0..array.length {
                            let elem = array.elements.add(i);
                            tryzub_free_value(elem);
                        }
                        dealloc(array.elements as *mut u8, 
                               Layout::array::<TryzubValue>(array.capacity).unwrap());
                    }
                }
                _ => {}
            }
        }
    }
}

// ===== Підтримка асинхронності =====

pub struct AsyncRuntime {
    thread_pool: ThreadPool,
    tasks: DashMap<usize, TaskState>,
    next_task_id: Arc<Mutex<usize>>,
}

#[derive(Debug)]
enum TaskState {
    Running,
    Completed(TryzubValue),
    Failed(TryzubError),
}

impl AsyncRuntime {
    pub fn new(num_threads: usize) -> Self {
        Self {
            thread_pool: ThreadPool::new(num_threads),
            tasks: DashMap::new(),
            next_task_id: Arc::new(Mutex::new(0)),
        }
    }
    
    pub fn spawn_task<F>(&self, task: F) -> Result<usize>
    where
        F: FnOnce() -> Result<TryzubValue> + Send + 'static,
    {
        let task_id = {
            let mut id = self.next_task_id.lock();
            let current_id = *id;
            *id += 1;
            current_id
        };
        
        self.tasks.insert(task_id, TaskState::Running);
        let tasks = self.tasks.clone();
        
        self.thread_pool.execute(move || {
            let result = task();
            match result {
                Ok(value) => {
                    tasks.insert(task_id, TaskState::Completed(value));
                }
                Err(e) => {
                    let error = TryzubError::new(ErrorKind::RuntimeError, e.to_string());
                    tasks.insert(task_id, TaskState::Failed(error));
                }
            }
        })?;
        
        Ok(task_id)
    }
    
    pub fn await_task(&self, task_id: usize) -> Result<TryzubValue> {
        loop {
            if let Some(state) = self.tasks.get(&task_id) {
                match state.value() {
                    TaskState::Running => {
                        thread::yield_now();
                        continue;
                    }
                    TaskState::Completed(value) => {
                        return Ok(value.clone());
                    }
                    TaskState::Failed(error) => {
                        return Err(anyhow::anyhow!(error.format_error()));
                    }
                }
            } else {
                return Err(anyhow::anyhow!("Завдання {} не знайдено", task_id));
            }
        }
    }
}

// ===== Глобальні функції runtime =====

static ASYNC_RUNTIME: Lazy<AsyncRuntime> = Lazy::new(|| {
    AsyncRuntime::new(num_cpus::get())
});

#[no_mangle]
pub extern "C" fn tryzub_allocate(size: usize) -> *mut c_void {
    unsafe {
        match MEMORY_MANAGER.allocate(size, 8, None) {
            Ok(ptr) => ptr as *mut c_void,
            Err(_) => ptr::null_mut(),
        }
    }
}

#[no_mangle]
pub extern "C" fn tryzub_deallocate(ptr: *mut c_void) {
    unsafe {
        let _ = MEMORY_MANAGER.deallocate(ptr as *mut u8);
    }
}

#[no_mangle]
pub extern "C" fn tryzub_get_memory_stats(total_allocated: *mut usize, allocation_count: *mut usize) {
    let stats = MEMORY_MANAGER.get_stats();
    unsafe {
        if !total_allocated.is_null() {
            *total_allocated = stats.total_allocated;
        }
        if !allocation_count.is_null() {
            *allocation_count = stats.allocation_count;
        }
    }
}

#[no_mangle]
pub extern "C" fn tryzub_spawn_async(callback: extern "C" fn() -> *mut TryzubValue) -> c_int {
    let result = ASYNC_RUNTIME.spawn_task(move || {
        unsafe {
            let value_ptr = callback();
            if value_ptr.is_null() {
                Err(anyhow::anyhow!("Асинхронна функція повернула null"))
            } else {
                Ok(*Box::from_raw(value_ptr))
            }
        }
    });
    
    match result {
        Ok(task_id) => task_id as c_int,
        Err(_) => -1,
    }
}

#[no_mangle]
pub extern "C" fn tryzub_await_async(task_id: c_int) -> *mut TryzubValue {
    if task_id < 0 {
        return ptr::null_mut();
    }
    
    match ASYNC_RUNTIME.await_task(task_id as usize) {
        Ok(value) => Box::into_raw(Box::new(value)),
        Err(_) => ptr::null_mut(),
    }
}

// Ініціалізація runtime
#[no_mangle]
pub extern "C" fn tryzub_runtime_init() -> c_int {
    // Форсуємо ініціалізацію lazy static
    let _ = &*MEMORY_MANAGER;
    let _ = &*ASYNC_RUNTIME;
    0
}

#[no_mangle]
pub extern "C" fn tryzub_runtime_shutdown() -> c_int {
    // Очищення ресурсів
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_memory_allocation() {
        unsafe {
            let ptr = MEMORY_MANAGER.allocate(1024, 8, Some("test".to_string())).unwrap();
            assert!(!ptr.is_null());
            
            MEMORY_MANAGER.deallocate(ptr).unwrap();
            
            let stats = MEMORY_MANAGER.get_stats();
            assert_eq!(stats.total_allocated, 0);
        }
    }
    
    #[test]
    fn test_thread_pool() {
        let pool = ThreadPool::new(4);
        let (tx, rx) = std::sync::mpsc::channel();
        
        for i in 0..10 {
            let tx = tx.clone();
            pool.execute(move || {
                tx.send(i).unwrap();
            }).unwrap();
        }
        
        let mut results = Vec::new();
        for _ in 0..10 {
            results.push(rx.recv().unwrap());
        }
        
        results.sort();
        assert_eq!(results, vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
    }
}

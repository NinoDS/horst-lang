use std::any::Any;
use std::collections::HashMap;
use std::fmt;
use std::marker::PhantomData;
use std::os::unix::process::parent_id;
use crate::class::{Class, ClassRef};
use crate::frame::CallFrame;
use crate::function::Function;
use crate::instance::Instance;
use crate::instruction::Instruction;
use crate::value::{InstanceRef, UpvalueRegistryRef, Value};

struct Heap {
    objects: HashMap<usize, Box<dyn Collectable>>,
    next_id: usize,
}


impl Heap {
    fn new() -> Heap {
        Heap {
            objects: HashMap::new(),
            next_id: 0,
        }
    }
}

pub trait Collectable: Any {
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn to_string(&self, _: &VM) -> Option<String> {
        None
    }
}

pub struct VM {
    stack: Vec<Value>,
    frames: Vec<CallFrame>,
    globals: HashMap<String, Value>,
    open_upvalues: Vec<UpvalueRegistryRef>,
    heap: Heap,
}

impl VM {
    pub fn new() -> VM {
        VM {
            stack: Vec::new(),
            frames: Vec::new(),
            globals: HashMap::new(),
            open_upvalues: Vec::new(),
            heap: Heap::new(),
        }
    }

    pub fn interpret(&mut self, function: Function) {
        let closure = Value::Closure(function, Vec::new());
        self.push(closure.clone());
        self.call_value(closure, 0);
        self.run();
    }

    fn run(&mut self) {
        macro_rules! binary_op {
            ($op:tt, $type:tt) => {
                let b = self.pop();
                let a = self.pop();

                if let (Value::Number(a), Value::Number(b)) = (a.clone(), b.clone()) {
                    self.push(Value::$type(a $op b ));
                } else {
                    panic!("Invalid operands for binary operation.");
                }
            };
        }

        loop {
            let instruction: Instruction = self.get_current_instruction();
            //dbg!(self.stack.clone());
            //dbg!(instruction.clone());
            self.frame_mut().ip += 1;

            match instruction {
                Instruction::Constant(index) => {
                    let constant = self.read_constant(index).clone();
                    self.stack.push(constant);
                }
                Instruction::Nil => self.stack.push(Value::Nil),
                Instruction::True => self.stack.push(Value::Boolean(true)),
                Instruction::False => self.stack.push(Value::Boolean(false)),
                Instruction::Pop => { self.stack.pop(); },
                Instruction::GetGlobal(index) => self.get_global(index),
                Instruction::DefineGlobal(index) => self.define_global(index),
                Instruction::SetGlobal(index) => self.set_global(index),
                Instruction::GetLocal(index) => self.get_local(index),
                Instruction::SetLocal(index) => self.set_local(index),
                Instruction::GetUpvalue(index) => self.get_upvalue(index),
                Instruction::SetUpvalue(index) => self.set_upvalue(index),
                Instruction::GetProperty(index) => self.get_property(index),
                Instruction::SetProperty(index) => self.set_property(index),
                Instruction::GetSuper(index) => self.get_super(index),
                Instruction::Equal => {
                    let b = self.stack.pop().unwrap();
                    let a = self.stack.pop().unwrap();
                    self.stack.push(Value::Boolean(a == b));
                }
                Instruction::Greater => { binary_op!(>, Boolean); },
                Instruction::Less => { binary_op!(<, Boolean); },
                Instruction::Subtract => { binary_op!(-, Number); },
                Instruction::Multiply => { binary_op!(*, Number); },
                Instruction::Divide => { binary_op!(/, Number); },
                Instruction::Add => {
                    let b = self.stack.pop().unwrap();
                    let a = self.stack.pop().unwrap();
                    match (a, b) {
                        (Value::Number(a), Value::Number(b)) => self.stack.push(Value::Number(a + b)),
                        (Value::String(a), Value::String(b)) => self.stack.push(Value::String(a + &b)),
                        _ => panic!("Operands must be two numbers or two strings."),
                    }
                }
                Instruction::Not => {
                    let value = self.stack.pop().unwrap();
                    self.stack.push(Value::Boolean(value.is_falsey()));
                }
                Instruction::Negate => {
                    let value = self.stack.pop().unwrap();
                    if let Value::Number(value) = value {
                        self.stack.push(Value::Number(-value));
                    } else {
                        panic!("Operand must be a number.");
                    }
                }
                Instruction::Print => {
                    println!("{}", self.stack.pop().unwrap());
                }
                Instruction::Jump(offset) => {
                    self.frame_mut().ip += offset;
                }
                Instruction::JumpIfFalse(offset) => {
                    let value = self.peek(0).unwrap();
                    if value.is_falsey() {
                        self.frame_mut().ip += offset;
                    }
                }
                Instruction::Loop(offset) => {
                    self.frame_mut().ip -= offset;
                }
                Instruction::Call(arg_count) => {
                    self.call_value_from_stack(arg_count);
                }
                Instruction::Invoke(index, arg_count) => {
                    let name = self.read_string(index);
                    self.invoke(name, arg_count);
                }
                Instruction::SuperInvoke(index, arg_count) => {
                    let name = self.read_string(index);
                    let superclass = self.stack.pop().unwrap();
                    match superclass {
                        Value::Class(class) => {
                            self.invoke_from_class(class, name, arg_count);
                        }
                        _ => panic!("Only classes have superclass."),
                    }
                }
                Instruction::Closure(index) => self.make_closure(index),
                Instruction::CloseUpvalue => {
                    let index = self.stack.len().checked_sub(1).unwrap();
                    self.close_upvalues(index);
                    self.stack.pop();
                }
                Instruction::Return => {
                    let base = self.frame().base;
                    let result = self.stack.pop().unwrap();
                    self.close_upvalues(base);
                    self.frames.pop();
                    if self.frames.is_empty() {
                        self.stack.pop();
                        return;
                    }
                    self.stack.truncate(base);
                    self.stack.push(result);
                }
                Instruction::Class(index) => {
                    let name = self.read_string(index);
                    let class = Class::new(name);
                    let value = self.new_class(class);
                    self.stack.push(value);
                }
                Instruction::Inherit => {
                    if let (Value::Class(mut subclass_ref), Value::Class(superclass_ref)) =
                        (self.stack.pop().unwrap(), self.peek(0).unwrap()) {
                        let superclass = self.get_class(*superclass_ref).unwrap().clone();
                        let mut subclass = self.get_class_mut(subclass_ref).unwrap();
                        for (name, method) in &superclass.methods {
                            subclass.methods.insert(name.clone(), method.clone());
                        }
                    } else {
                        panic!("Superclass must be a class.");
                    }
                }
                Instruction::Method(index) => self.define_method(index),
            }
        }
    }

    fn get_current_instruction(&self) -> Instruction {
        let frame = self.frame();
        frame.chunk().code[frame.ip].clone()
    }

    fn frame(&self) -> &CallFrame {
        self.frames.last().unwrap()
    }

    fn frame_mut(&mut self) -> &mut CallFrame {
        self.frames.last_mut().unwrap()
    }

    fn read_constant(&self, index: usize) -> &Value {
        let frame = self.frame();
        frame.chunk().read_constant(index)
    }

    fn peek(&self, distance: usize) -> Option<&Value> {
        let index = self.stack.len().checked_sub(1 + distance)?;
        self.stack.get(index)
    }

    fn read_string(&self, index: usize) -> String {
        let value = self.read_constant(index);
        match value {
            Value::String(string) => string.clone(),
            _ => panic!("Value is not a string."),
        }
    }

    fn get_global(&mut self, index: usize) {
        let name = self.read_string(index);
        if let Some(value) = self.globals.get(&name) {
            self.stack.push(value.clone());
        } else {
            panic!("Undefined variable '{}'.", name);
        }
    }

    fn define_global(&mut self, index: usize) {
        let name = self.read_string(index);
        let value = self.stack.pop().unwrap();
        self.globals.insert(name, value);
    }

    fn set_global(&mut self, index: usize) {
        let name = self.read_string(index);
        if self.globals.contains_key(&name) {
            let value = self.peek(0).unwrap().clone();
            self.globals.insert(name, value);
        } else {
            panic!("Undefined variable '{}'.", name);
        }
    }

    fn get_local(&mut self, index: usize) {
        let base = self.frame().base;
        let value = self.stack[base + index].clone();
        self.stack.push(value);
    }

    fn set_local(&mut self, index: usize) {
        let base = self.frame().base;
        let value = self.peek(0).unwrap().clone();
        self.stack[base + index] = value;
    }

    fn make_closure(&mut self, index: usize) {
        let constant = self.read_constant(index).clone();
        if let Value::Function(function) = constant {
            let mut upvalues = Vec::new();
            let upvals = function.upvalues.clone();
            for FunctionUpvalue { is_local, index } in upvals {
                upvalues.push(if is_local {
                    self.capture_upvalue(self.frame().base + index)
                } else {
                    self.frame().get_upvalue(index)
                });
            }

            let closure = Value::Closure(function, upvalues);
            self.stack.push(closure);
        } else {
            panic!("Value is not a function.");
        }
    }

    fn capture_upvalue(&mut self, index: usize) -> UpvalueRegistryRef {
        if let Some(upvalue) = self.open_upvalues.iter().find(|upvalue| matches!(self.get_collectable::<UpvalueRegistry>(**upvalue), Some(&UpvalueRegistry::Open(i)) if i == index)) {
            upvalue.clone()
        } else {
            let upvalue = UpvalueRegistry::Open(index);
            let upvalue_ref = self.new_collectable(upvalue);
            self.open_upvalues.push(upvalue_ref);
            upvalue_ref
        }
    }

    fn get_upvalue(&mut self, index: usize) {
        let upvalue_ref = self.frame().get_upvalue(index);
        let upvalue = self.get_collectable::<UpvalueRegistry>(upvalue_ref).unwrap().clone();
        match upvalue {
            UpvalueRegistry::Open(index) => {
                let value = self.stack[index].clone();
                self.stack.push(value);
            }
            UpvalueRegistry::Closed(value) => {
                self.stack.push(value);
            }
        }
    }

    fn set_upvalue(&mut self, index: usize) {
        let value = self.peek(0).unwrap().clone();
        let upvalue_ref = self.frame_mut().get_upvalue(index);
        let upvalue = self.get_collectable_mut::<UpvalueRegistry>(upvalue_ref).unwrap();
        match upvalue {
            UpvalueRegistry::Open(index) => {
                let index = *index;
                self.stack[index] = value;
            }
            UpvalueRegistry::Closed(ref mut cell) => {
                *cell = value;
            }
        }
    }

    fn close_upvalues(&mut self, index: usize) {
        while let Some(upvalue_ref) = self.open_upvalues.last() {
            let upvalue = self.get_collectable::<UpvalueRegistry>(*upvalue_ref).unwrap();
            let slot = if let UpvalueRegistry::Open(slot) = upvalue {
                if *slot <= index {
                    break;
                }
                *slot
            } else {
                panic!("Expected open upvalue.");
            };
            let upvalue_ref = self.open_upvalues.pop().unwrap();
            let value = self.stack[slot].clone();
            let upvalue = self.get_collectable_mut::<UpvalueRegistry>(upvalue_ref).unwrap();
            upvalue.close(value);
        }
    }

    fn get_property(&mut self, index: usize) {
        let name = self.read_string(index);
        let instance = self.stack.pop().unwrap();
        match instance {
            Value::Instance(instance_ref) => {
                let instance = self.get_collectable::<Instance>(instance_ref).unwrap().clone();
                if let Some(value) = instance.fields.get(&name) {
                    self.stack.push(value.clone());
                } else {
                    self.bind_method(instance.class, instance_ref, name);
                }
            }
            _ => panic!("Only instances have properties."),
        }
    }

    fn bind_method(&mut self, class: ClassRef, instance: InstanceRef, name: String) {
        let class = self.get_class(class).unwrap().clone();
        if let Some(method) = class.methods.get(&name) {
            let (function, upvalues) = match method {
                Value::Function(f) => (f, Vec::new()),
                Value::Closure(f, u) => (f, u.clone()),
                _ => panic!("Expected function or closure."),
            };

            self.stack.push(Value::BoundMethod {
                receiver: instance,
                function: function.clone(),
                upvalues,
            });
        } else {
            panic!("Undefined property '{}'.", name);
        }
    }

    fn set_property(&mut self, index: usize) {
        let name = self.read_string(index);
        let value = self.pop().clone();
        let instance = self.pop().clone();

        match instance {
            Value::Instance(instance_ref) => {
                let mut instance = self.get_collectable_mut::<Instance>(instance_ref).unwrap();
                instance.fields.insert(name, value.clone());
            }
            _ => panic!("Only instances have fields."),
        }
        self.push(value);
    }

    fn get_super(&mut self, index: usize) {
        let (this_val, super_val) = (self.stack.pop().unwrap(), self.stack.pop().unwrap());
        if let (Value::Class(super_class), Value::Instance(this)) = (super_val, this_val) {
            let name = self.read_string(index);
            self.bind_method(super_class, this, name);
        } else {
            panic!("Superclass must be a class.")
        }
    }

    fn define_method(&mut self, index: usize) {
        let method = self.stack.pop().unwrap();
        let class = self.peek(0).unwrap().clone();
        if let Value::Class(class) = class {
            let name = self.read_string(index);
            let class = self.get_class_mut(class).unwrap();
            class.methods.insert(name, method);
        } else {
            panic!("Expected class.");
        }
    }

    fn invoke(&mut self, method: String, arg_count: usize) {
        let receiver = self.peek(arg_count).unwrap().clone();
        match receiver {
            Value::Instance(instance_ref) => {
                let instance = self.get_collectable::<Instance>(instance_ref).unwrap().clone();
                if let Some(method) = instance.fields.get(&method) {
                    let l = self.stack.len();
                    self.stack[l - arg_count - 1] = method.clone();
                    self.call_value_from_stack(arg_count);
                } else {
                    let class = instance.class;
                    self.invoke_from_class(class, method, arg_count);
                }
            }
            _ => panic!("Only instances have methods."),
        }
    }

    fn invoke_from_class(&mut self, class: ClassRef, method: String, arg_count: usize) {
        let class = self.get_class(class).unwrap().clone();
        if let Some(method) = class.methods.get(&method) {
            self.call_value(method.clone(), arg_count);
        } else {
            panic!("Undefined property '{}'.", method);
        }
    }

    fn call_value(&mut self, callee: Value, arg_count: usize) {
        match callee {
            Value::Closure(function, upvalues) => {
                self.call(function, upvalues, arg_count);
            }
            Value::Function(function) => {
                self.call(function, Vec::new(), arg_count);
            }
            Value::Class(class) => {
                let instance = Instance::new(class.clone());
                let instance_ref = self.new_collectable(instance);
                let value = Value::Instance(instance_ref);
                let l = self.stack.len();
                self.stack[l - arg_count - 1] = value;

                let class = self.get_class(class).unwrap().clone();
                if let Some(init) = class.methods.get("init") {
                    match init {
                        Value::Closure(function, upvalues) => {
                            self.call(function.clone(), upvalues.clone(), arg_count);
                        }
                        Value::Function(function) => {
                            self.call(function.clone(), Vec::new(), arg_count);
                        }
                        _ => panic!("Expected function."),
                    }
                } else if arg_count != 0 {
                    panic!("Expected 0 arguments but got {}.", arg_count);
                }
            }
            Value::BoundMethod {
                receiver,
                function,
                upvalues,
            } => {
                let l = self.stack.len();
                self.stack[l - arg_count - 1] = Value::Instance(receiver);
                self.call(function, upvalues, arg_count);
            }
            Value::NativeFunction(function) => {
                let from = self.stack.len() - arg_count;
                let args = self.stack[from..].to_vec();
                let result = (function.function)(args, self);
                self.pop_many(arg_count + 1);
                self.stack.push(result);
            }
            _ => panic!("Can only call functions and classes."),
        }
    }

    fn call_value_from_stack(&mut self, arg_count: usize) {
        let callee = self.peek(arg_count).unwrap().clone();
        self.call_value(callee, arg_count);
    }

    fn call(&mut self, function: Function, upvalues: Vec<UpvalueRegistryRef>, arg_count: usize) {
        if arg_count != function.arity {
            panic!(
                "Expected {} arguments but got {}.",
                function.arity, arg_count
            );
        }
        self.frames.push(CallFrame {
            function,
            ip: 0,
            base: self.stack.len() - arg_count - 1,
            upvalues,
        });
    }

    fn pop_many(&mut self, count: usize) {
        for _ in 0..count {
            self.stack.pop();
        }
    }

    fn pop(&mut self) -> Value {
        self.stack.pop().unwrap()
    }

    fn push(&mut self, value: Value) {
        self.stack.push(value);
    }

    pub fn new_instance(&mut self, instance: Instance) -> Value {
        let id = self.heap.next_id;
        self.heap.next_id += 1;
        self.heap.objects.insert(id, Box::new(instance));
        Value::Instance(id)
    }

    pub fn get_instance(&self, id: usize) -> Option<&Instance> {
        match self.heap.objects.get(&id) {
            Some(collectable) => collectable.as_any().downcast_ref::<Instance>(),
            None => None,
        }
    }

    pub(crate) fn get_instance_mut(&mut self, id: usize) -> Option<&mut Instance> {
        match self.heap.objects.get_mut(&id) {
            Some(collectable) => collectable.as_any_mut().downcast_mut::<Instance>(),
            None => None,
        }
    }

    pub(crate) fn new_class(&mut self, class: Class) -> Value {
        let id = self.heap.next_id;
        self.heap.next_id += 1;
        self.heap.objects.insert(id, Box::new(class));
        Value::Class(id)
    }

    pub(crate) fn get_class(&self, id: usize) -> Option<&Class> {
        match self.heap.objects.get(&id) {
            Some(collectable) => collectable.as_any().downcast_ref::<Class>(),
            None => None,
        }
    }

    pub(crate) fn get_class_mut(&mut self, id: usize) -> Option<&mut Class> {
        match self.heap.objects.get_mut(&id) {
            Some(collectable) => collectable.as_any_mut().downcast_mut::<Class>(),
            None => None,
        }
    }

    pub fn get_collectable<T: Collectable>(&self, id: usize) -> Option<&T> {
        match self.heap.objects.get(&id) {
            Some(collectable) => collectable.as_any().downcast_ref::<T>(),
            None => None,
        }
    }

    pub fn get_collectable_mut<T: Collectable>(&mut self, id: usize) -> Option<&mut T> {
        match self.heap.objects.get_mut(&id) {
            Some(collectable) => collectable.as_any_mut().downcast_mut::<T>(),
            None => None,
        }
    }

    pub fn new_collectable<T: Collectable>(&mut self, collectable: T) -> usize {
        let id = self.heap.next_id;
        self.heap.next_id += 1;
        self.heap.objects.insert(id, Box::new(collectable));
        id
    }

}

#[derive(Clone)]
pub enum UpvalueRegistry {
    Open(usize),
    Closed(Value),
}

impl Collectable for UpvalueRegistry {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl UpvalueRegistry {
    fn close(&mut self, value: Value) {
        *self = UpvalueRegistry::Closed(value);
    }
}

impl fmt::Debug for UpvalueRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UpvalueRegistry::Open(index) => write!(f, "UpvalueRef::Open({})", index),
            UpvalueRegistry::Closed(value) => write!(f, "UpvalueRef::Closed({:?})", value),
        }
    }
}

impl PartialEq for UpvalueRegistry {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (UpvalueRegistry::Open(index1), UpvalueRegistry::Open(index2)) => index1 == index2,
            (UpvalueRegistry::Closed(value1), UpvalueRegistry::Closed(value2)) => value1 == value2,
            _ => false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FunctionUpvalue {
    pub index: usize,
    pub is_local: bool,
}
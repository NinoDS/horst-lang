use std::collections::HashMap;
use std::ops::{Add, Deref};
use crate::class::Class;
use crate::function::{Function, NativeFunction};
use crate::value::Value;
use crate::vm::{VM};
use rand::Rng;

pub fn make_readln() -> NativeFunction {
    return NativeFunction { function: readln };
}

pub fn make_number() -> NativeFunction {
    return NativeFunction { function: number };
}

pub fn make_int() -> NativeFunction { return NativeFunction { function: int }; }

pub fn make_random() -> NativeFunction {
    return NativeFunction { function: random };
}

pub fn make_floor() -> NativeFunction { return NativeFunction { function: floor }; }

pub fn make_panic() -> NativeFunction { return NativeFunction { function: panic }; }

fn readln(_: Vec<Value>, vm: &mut VM) -> Value {
    let mut s = String::new();
    std::io::stdin().read_line(&mut s).unwrap_or_else(|_| {
        vm.error("Could not read line");
    });
    s.pop();
    Value::String(s)
}

fn random(_: Vec<Value>, vm: &mut VM) -> Value {
    let mut rng = rand::thread_rng();
    Value::Number(rng.gen_range(0.0..1.0))
}

fn number(args: Vec<Value>, vm: &mut VM) -> Value {
    let mut args = args;
    let s = if let Some(Value::String(s)) = args.pop() {
        s
    } else {
        vm.error("First argument must be a string");
    };
    if let Ok(number) = s.parse::<f64>() {
        return Value::Number(number);
    }
    Value::Nil
}

fn int(args: Vec<Value>, vm: &mut VM) -> Value {
    let mut args = args;
    let s = if let Some(Value::String(s)) = args.pop() {
        s
    } else {
        vm.error("First argument must be a string");
    };
    if let Ok(number) = s.parse::<i32>() {
        return Value::Number(number as f64);
    }
    Value::Nil
}

fn floor(args: Vec<Value>, vm: &mut VM) -> Value {
    let mut args = args;
    let number = if let Some(Value::Number(number)) = args.pop() {
        number
    } else {
        vm.error("First argument must be a number");
    };
    Value::Number(number.floor())
}

fn panic(args: Vec<Value>, vm: &mut VM) -> Value {
    let mut args = args;
    let message = if let Some(Value::String(message)) = args.pop() {
        message
    } else {
        vm.error("First argument must be a string");
    };
    vm.error(message);
}

fn make_map() -> Class {
    let mut methods = HashMap::new();
    methods.insert("get".to_string(), Value::NativeFunction(NativeFunction { function: map_get }));
    methods.insert("set".to_string(), Value::NativeFunction(NativeFunction { function: map_set }));
    methods.insert("toString".to_string(), Value::NativeFunction(NativeFunction { function: map_to_string }));
    Class {
        name: "Map".to_string(),
        methods,
    }
}

fn map_get(args: Vec<Value>, vm: &mut VM) -> Value {
    let mut args = args;
    let map = if let Value::Instance(map) = args.remove(0) {
        vm.gc.deref(map)
    } else {
        panic!("First argument must be a map");
    };
    let key = if let Value::String(key) = args.remove(0) {
        key
    } else {
        panic!("Second argument must be a string");
    };
    map.fields.get(&key).unwrap_or(&Value::Nil).clone()
}

fn map_set(args: Vec<Value>, vm: &mut VM) -> Value {
    println!("{:?}", args);
    let mut args = args;
    let mut map = if let Value::Instance(map) = args.remove(0) {
        vm.gc.deref_mut(map)
    } else {
        panic!("First argument must be a map");
    };
    let key = if let Value::String(key) = args.remove(0) {
        key
    } else {
        panic!("Second argument must be a string");
    };
    let value = args.pop().unwrap();
    map.fields.insert(key, value);
    Value::Nil
}

fn map_to_string(args: Vec<Value>, vm: &mut VM) -> Value {
    let mut args = args;
    let map = if let Value::Instance(map) = args.pop().unwrap() {
        vm.gc.deref(map)
    } else {
        panic!("First argument must be a map");
    };
    let mut s = "{".to_string();
    for (i, (key, value)) in map.fields.iter().enumerate() {
        if i > 0 {
            s.push_str(", ");
        }
        if let Value::String(value) = value {
            s.push_str(&format!("{}: \"{}\"", key, value));
        } else {
            s.push_str(&format!("{}: {}", key, value));
        }
    }
    s.push('}');
    Value::String(s)
}
// src-tauri/src/streams.rs（Task 2 先建骨架，Task 3 补 StreamTask/WS 逻辑）
use std::collections::HashMap;
use std::sync::Mutex;

pub struct StreamRegistry {
    pub tasks: Mutex<HashMap<u64, ()>>,
    next_id: u64,
}

impl StreamRegistry {
    pub fn new() -> Self {
        Self { tasks: Mutex::new(HashMap::new()), next_id: 1 }
    }
    pub fn next_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }
}

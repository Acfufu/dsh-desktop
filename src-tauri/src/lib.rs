mod http_command;
mod streams;
mod process;
mod state_machine;

use std::sync::{Arc, Mutex};

// R5 修正：AppState 唯一真源（Task 2 起）；http_command.rs 只 use crate::AppState
pub struct AppState {
    pub http_client: reqwest::Client,
    pub uds_path: String,
    pub registry: Arc<Mutex<streams::StreamRegistry>>,
}

impl Clone for AppState {
    fn clone(&self) -> Self {
        Self {
            http_client: self.http_client.clone(),
            uds_path: self.uds_path.clone(),
            registry: Arc::clone(&self.registry),
        }
    }
}

pub fn run() {
    // Task 3 Step 4 补完整 Builder chain
}

use std::sync::Arc;

use crate::executor::{AgentExecutor, ExecutorConfig};

pub struct ExecutorPool {
    executor: Arc<AgentExecutor>,
}

impl ExecutorPool {
    pub fn new(config: ExecutorConfig) -> Self {
        Self {
            executor: Arc::new(AgentExecutor::new(config)),
        }
    }

    pub fn executor(&self) -> Arc<AgentExecutor> {
        self.executor.clone()
    }
}

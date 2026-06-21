use std::collections::HashMap;

use gpui::Entity;

use crate::core::session_handle::SessionHandle;

#[derive(Default)]
pub struct SessionManager {
    sessions: HashMap<String, Entity<SessionHandle>>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, session_file: &str) -> Option<Entity<SessionHandle>> {
        self.sessions.get(session_file).cloned()
    }

    pub fn register(&mut self, session_file: String, handle: Entity<SessionHandle>) {
        self.sessions.insert(session_file, handle);
    }
}

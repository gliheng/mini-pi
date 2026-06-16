use std::collections::HashMap;

use gpui::Entity;

use crate::core::session_handle::SessionHandle;

pub struct SessionManager {
    sessions: HashMap<String, Entity<SessionHandle>>,
}

impl Default for SessionManager {
    fn default() -> Self {
        Self {
            sessions: HashMap::new(),
        }
    }
}

impl SessionManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, session_file: &str) -> Option<Entity<SessionHandle>> {
        self.sessions.get(session_file).cloned()
    }

    pub fn register(
        &mut self,
        session_file: String,
        handle: Entity<SessionHandle>,
    ) {
        self.sessions.insert(session_file, handle);
    }
}

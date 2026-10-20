use std::collections::BTreeMap;

use async_trait::async_trait;
use tower_sessions::{
    session::{Id, Record},
    session_store,
};

use super::Shared;
use crate::store::SessionData;

pub struct MemSessionStore {
    pub(super) shared: std::sync::Arc<Shared>,
}

#[async_trait]
impl SessionData for MemSessionStore {
    async fn create(&self, record: &mut Record) -> session_store::Result<()> {
        let mut tables = self.shared.lock();
        while tables.sessions.contains_key(&record.id.to_string()) {
            record.id = Id::default();
        }
        insert(&mut tables.sessions, record);
        Ok(())
    }

    async fn save(&self, record: &Record) -> session_store::Result<()> {
        insert(&mut self.shared.lock().sessions, record);
        Ok(())
    }

    async fn load(&self, id: &Id) -> session_store::Result<Option<Record>> {
        let now = self.shared.now();
        Ok(self
            .shared
            .lock()
            .sessions
            .get(&id.to_string())
            .filter(|record| record.expiry_date > now)
            .cloned())
    }

    async fn delete(&self, id: &Id) -> session_store::Result<()> {
        self.shared.lock().sessions.remove(&id.to_string());
        Ok(())
    }

    async fn delete_expired(&self) -> session_store::Result<()> {
        let now = self.shared.now();
        self.shared
            .lock()
            .sessions
            .retain(|_, record| record.expiry_date >= now);
        Ok(())
    }
}

fn insert(sessions: &mut BTreeMap<String, Record>, record: &Record) {
    let mut stored = record.clone();
    stored.expiry_date = super::micros(stored.expiry_date);
    sessions.insert(record.id.to_string(), stored);
}

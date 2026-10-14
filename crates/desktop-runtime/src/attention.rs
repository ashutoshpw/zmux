//! Bounded in-app attention. Signals carry identity; terminal text is never interpreted as a command.
use serde::{Deserialize, Serialize};
use zmux_workspace_core::SurfaceId;

pub const MAX_NOTIFICATIONS: usize = 128;
#[derive(Clone, Debug, Serialize)]
pub struct Notification {
    pub id: u64,
    pub surface_id: SurfaceId,
    pub title: String,
    pub body: String,
    pub source: String,
    pub timestamp_ms: u64,
    pub read: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum AttentionCommand {
    Notify {
        surface_id: SurfaceId,
        title: String,
        body: String,
    },
    Read {
        id: u64,
    },
    Open {
        id: u64,
    },
    Dismiss {
        id: u64,
    },
    /// A cursor is mandatory: a newly arrived generation must survive a stale clear.
    Clear {
        through_id: u64,
    },
    AppActive {
        active: bool,
    },
}
#[derive(Default)]
pub(crate) struct Attention {
    pub items: Vec<Notification>,
    pub dropped: u64,
    next_id: u64,
}
impl Attention {
    pub fn notify(
        &mut self,
        surface_id: SurfaceId,
        source: &str,
        title: &str,
        body: &str,
        read: bool,
    ) -> Result<(), String> {
        let title = clean(title, 128);
        let body = clean(body, 2048);
        if title.is_empty() {
            return Err("notification title must not be empty".into());
        }
        let timestamp_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .min(u128::from(u64::MAX)) as u64;
        // Replace an unread repeat with a new generation. A stale acknowledgement must
        // never mark a newly arrived repeat read, even while the UI coalesces its row.
        if self.items.last().is_some_and(|last| {
            !last.read
                && !read
                && last.surface_id == surface_id
                && last.source == source
                && last.title == title
                && last.body == body
                && timestamp_ms.saturating_sub(last.timestamp_ms) < 1000
        }) {
            self.items.pop();
        }
        let id = self
            .next_id
            .checked_add(1)
            .ok_or("notification identity space exhausted")?;
        self.next_id = id;
        if self.items.len() == MAX_NOTIFICATIONS {
            self.items.remove(0);
            self.dropped = self.dropped.saturating_add(1);
        }
        self.items.push(Notification {
            id,
            surface_id,
            title,
            body,
            source: source.into(),
            timestamp_ms,
            read,
        });
        Ok(())
    }
    pub fn find(&self, id: u64) -> Result<&Notification, String> {
        self.items
            .iter()
            .find(|n| n.id == id)
            .ok_or_else(|| format!("notification {id} does not exist"))
    }
    pub fn read(&mut self, id: u64) -> Result<(), String> {
        let item = self
            .items
            .iter_mut()
            .find(|n| n.id == id)
            .ok_or_else(|| format!("notification {id} does not exist"))?;
        item.read = true;
        Ok(())
    }
    pub fn dismiss(&mut self, id: u64) -> Result<(), String> {
        self.find(id)?;
        self.items.retain(|n| n.id != id);
        Ok(())
    }
    pub fn clear(&mut self, through: u64) {
        self.items.retain(|n| n.id > through);
    }
}
fn clean(value: &str, max: usize) -> String {
    value
        .chars()
        .filter(|c| {
            !c.is_control() && !matches!(*c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
        .take(max)
        .collect::<String>()
        .trim()
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_history_reports_eviction_and_stale_ack_cannot_consume_new_generations() {
        let mut state = Attention::default();
        for index in 0..MAX_NOTIFICATIONS + 3 {
            state
                .notify(SurfaceId(1), "test", &format!("Event {index}"), "", false)
                .unwrap();
        }
        assert_eq!(state.items.len(), MAX_NOTIFICATIONS);
        assert_eq!(state.dropped, 3);
        let first = state.items[0].id;
        let last = state.items.last().unwrap().id;
        state.read(first).unwrap();
        assert!(!state.find(last).unwrap().read);
        state.clear(first);
        assert!(state.find(last).is_ok());
        assert!(state.read(first).is_err());
        state
            .notify(SurfaceId(1), "test", "Duplicate", "", false)
            .unwrap();
        let size = state.items.len();
        let old_id = state.items.last().unwrap().id;
        state
            .notify(SurfaceId(1), "test", "Duplicate", "", false)
            .unwrap();
        assert_eq!(state.items.len(), size);
        assert!(state.read(old_id).is_err());
        assert!(!state.items.last().unwrap().read);
    }
}

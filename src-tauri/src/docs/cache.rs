use parking_lot::Mutex;
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use super::model::DocBundle;

const TTL: Duration = Duration::from_secs(30 * 60);
const MAX_DRAFTS: usize = 3;

struct Cached {
    bundle: DocBundle,
    created: Instant,
}

#[derive(Default)]
struct Inner {
    drafts: HashMap<String, Cached>,
    order: VecDeque<String>,
}

impl Inner {
    fn purge(&mut self) {
        let now = Instant::now();
        let expired: Vec<String> = self
            .drafts
            .iter()
            .filter(|(_, item)| now.duration_since(item.created) > TTL)
            .map(|(id, _)| id.clone())
            .collect();
        for id in expired {
            self.drafts.remove(&id);
        }
        self.order.retain(|id| self.drafts.contains_key(id));
    }
}

/// 接口文档草稿缓存：draft_id → IR，30 分钟过期，最多 3 份，超出按写入顺序淘汰。
#[derive(Default)]
pub struct DocsCache {
    inner: Mutex<Inner>,
}

impl DocsCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn put(&self, bundle: DocBundle) -> String {
        let mut inner = self.inner.lock();
        inner.purge();
        let id = uuid::Uuid::new_v4().to_string();
        inner.drafts.insert(
            id.clone(),
            Cached {
                bundle,
                created: Instant::now(),
            },
        );
        inner.order.push_back(id.clone());
        while inner.order.len() > MAX_DRAFTS {
            if let Some(old) = inner.order.pop_front() {
                inner.drafts.remove(&old);
            }
        }
        id
    }

    pub fn get(&self, id: &str) -> Result<DocBundle, String> {
        let mut inner = self.inner.lock();
        inner.purge();
        inner
            .drafts
            .get(id)
            .map(|item| item.bundle.clone())
            .ok_or_else(|| "E_DRAFT_EXPIRED: 草稿已过期，请重新生成".to_string())
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.inner.lock().drafts.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::docs::model::*;

    fn bundle(tag: &str) -> DocBundle {
        DocBundle {
            generated_at: 0,
            app_version: "1".into(),
            revision: tag.into(),
            subtitle: String::new(),
            include_generated_at: false,
            page_break_per_request: true,
            scope: DocScopeInfo {
                kind: "project".into(),
                title: tag.into(),
                project_name: "P".into(),
                target_id: 1,
                environment_id: None,
                environment_name: None,
            },
            groups: vec![],
            environments: vec![],
            warnings: vec![],
        }
    }

    #[test]
    fn stores_gets_and_reports_expired() {
        let cache = DocsCache::new();
        let id = cache.put(bundle("x"));
        assert_eq!(cache.get(&id).unwrap().revision, "x");
        assert!(cache
            .get("missing")
            .unwrap_err()
            .contains("E_DRAFT_EXPIRED"));
    }

    #[test]
    fn evicts_oldest_beyond_three() {
        let cache = DocsCache::new();
        let first = cache.put(bundle("1"));
        let _ = cache.put(bundle("2"));
        let _ = cache.put(bundle("3"));
        let _ = cache.put(bundle("4"));
        assert_eq!(cache.len(), 3);
        assert!(cache.get(&first).is_err());
    }
}

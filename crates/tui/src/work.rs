//! Coalesce rapid navigation into one in-flight query and one latest pending query.
//! Older results can never relabel a newer page's content.
#[derive(Default, Debug)]
pub struct LatestRequest {
    revision: u64,
    active: Option<u64>,
    pending: bool,
}
impl LatestRequest {
    /// Record that the current view needs fresh data.
    pub fn request(&mut self) {
        self.revision += 1;
        self.pending = true;
    }
    /// Claim the pending request when no query is in flight.
    pub fn begin(&mut self) -> Option<u64> {
        if self.active.is_some() || !self.pending {
            return None;
        }
        self.pending = false;
        self.active = Some(self.revision);
        self.active
    }
    /// Complete a query; returns whether its result still describes the current view.
    pub fn finish(&mut self, revision: u64) -> bool {
        if self.active != Some(revision) {
            return false;
        }
        self.active = None;
        revision == self.revision
    }
    pub fn loading(&self) -> bool {
        self.active.is_some() || self.pending
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn coalesces_navigation_and_rejects_stale_results() {
        let mut state = LatestRequest::default();
        state.request();
        let first = state.begin().unwrap();
        for _ in 0..100 {
            state.request();
            assert_eq!(state.begin(), None);
        }
        assert!(!state.finish(first));
        let latest = state.begin().unwrap();
        assert!(latest > first);
        assert!(state.finish(latest));
        assert!(!state.loading());
    }
    #[test]
    fn unknown_revisions_are_ignored() {
        let mut state = LatestRequest::default();
        assert!(!state.finish(42));
        assert!(!state.loading());
        state.request();
        assert!(state.loading());
        let id = state.begin().unwrap();
        assert!(!state.finish(id + 1));
        assert!(state.finish(id));
    }
}

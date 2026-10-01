/// Coalesce rapid navigation into one in-flight query and one latest pending query.
/// Older results can never relabel a newer page's content.
#[derive(Default)]
pub struct LatestRequest {
    /// Advanced by navigation; a result for an older view is never shown.
    view: u64,
    active: Option<u64>,
    pending: bool,
    /// The view whose result was shown last.
    shown: Option<u64>,
}
impl LatestRequest {
    /// The view changed (another page, folder, sort or page of results): results for older views
    /// are stale.
    pub fn request(&mut self) {
        self.view += 1;
        self.pending = true;
    }
    /// The same view needs newer data, as while a scan fills it. A result already on its way still
    /// describes this view, so it is shown and the refresh follows; a slow query can no longer be
    /// discarded forever by refreshes that arrive faster than it answers.
    pub fn refresh(&mut self) {
        self.pending = true;
    }
    /// Claim the pending request when no query is in flight.
    pub fn begin(&mut self) -> Option<u64> {
        if self.active.is_some() || !self.pending {
            return None;
        }
        self.pending = false;
        self.active = Some(self.view);
        self.active
    }
    /// Complete a query; returns whether its result still describes the current view.
    pub fn finish(&mut self, revision: u64) -> bool {
        if self.active != Some(revision) {
            return false;
        }
        self.active = None;
        let current = revision == self.view;
        if current {
            self.shown = Some(revision);
        }
        current
    }
    /// A query is in flight or waiting to start.
    pub fn loading(&self) -> bool {
        self.active.is_some() || self.pending
    }
    /// No result has been shown for the current view yet: show a placeholder, not an empty state.
    pub fn waiting(&self) -> bool {
        self.shown != Some(self.view)
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
    fn refreshes_of_the_same_view_keep_the_result_on_its_way() {
        let mut state = LatestRequest::default();
        state.request();
        let first = state.begin().unwrap();
        assert!(state.waiting());
        // A scan keeps asking for fresher data while the first answer is still coming.
        for _ in 0..10 {
            state.refresh();
            assert_eq!(state.begin(), None);
        }
        assert!(state.finish(first), "a refresh must not make the answer on its way stale");
        assert!(!state.waiting());
        assert!(state.loading(), "the refresh is still pending");
        let second = state.begin().unwrap();
        assert!(state.finish(second));
        assert!(!state.loading());
        // Navigation still discards an answer meant for the previous view.
        state.request();
        let third = state.begin().unwrap();
        state.request();
        assert!(!state.finish(third));
        assert!(state.waiting());
    }
}

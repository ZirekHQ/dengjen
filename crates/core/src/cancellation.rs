use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Clone, Default)]
pub struct CancellationToken(Arc<AtomicBool>);

impl CancellationToken {
    pub fn new() -> Self {
        Self(Arc::new(AtomicBool::new(false)))
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }

    pub fn points_to_same_flag(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_token_is_not_cancelled() {
        let token = CancellationToken::new();
        assert!(!token.is_cancelled());
    }

    #[test]
    fn cancel_marks_token_cancelled() {
        let token = CancellationToken::new();
        token.cancel();
        assert!(token.is_cancelled());
    }

    #[test]
    fn clones_share_cancellation_state() {
        let token = CancellationToken::new();
        let clone = token.clone();
        clone.cancel();
        assert!(
            token.is_cancelled(),
            "cancelling a clone must be visible on the original"
        );
    }

    #[test]
    fn default_token_is_not_cancelled() {
        assert!(!CancellationToken::default().is_cancelled());
    }

    #[test]
    fn distinct_tokens_do_not_point_to_the_same_flag() {
        let a = CancellationToken::new();
        let b = CancellationToken::new();
        assert!(!a.points_to_same_flag(&b));
    }

    #[test]
    fn a_token_and_its_clone_point_to_the_same_flag() {
        let token = CancellationToken::new();
        let clone = token.clone();
        assert!(token.points_to_same_flag(&clone));
    }
}

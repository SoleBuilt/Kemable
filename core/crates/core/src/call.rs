use std::future::Future;
use std::time::{Duration, Instant};

use tokio_util::sync::CancellationToken;

use crate::DriverError;

/// What every call into a connection carries: when it must end, and a token
/// that stops it sooner.
///
/// A driver uses the token to stop the statement on the server (SQLite's
/// interrupt, PostgreSQL's cancel request, a TDS attention), because dropping
/// the future alone leaves the statement running there. [`CallOptions::run`]
/// is the fallback for calls with no server-side stop.
#[derive(Debug, Clone, Default)]
pub struct CallOptions {
    pub deadline: Option<Instant>,
    pub cancel: CancellationToken,
}

impl CallOptions {
    pub fn new(cancel: CancellationToken) -> Self {
        Self { deadline: None, cancel }
    }

    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.deadline = Some(Instant::now() + timeout);
        self
    }

    /// A child for one step of a larger call: the parent's cancel reaches it,
    /// its own cancel does not reach the parent.
    #[must_use]
    pub fn child(&self) -> Self {
        Self {
            deadline: self.deadline,
            cancel: self.cancel.child_token(),
        }
    }

    pub fn remaining(&self) -> Option<Duration> {
        self.deadline
            .map(|deadline| deadline.saturating_duration_since(Instant::now()))
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }

    pub fn is_expired(&self) -> bool {
        self.deadline.is_some_and(|deadline| Instant::now() >= deadline)
    }

    /// Fails when the call is cancelled or past its deadline. Drivers check
    /// it between stages of work that cannot be interrupted.
    pub fn check(&self) -> Result<(), DriverError> {
        if self.is_cancelled() {
            return Err(DriverError::cancelled());
        }
        if self.is_expired() {
            return Err(DriverError::timeout());
        }
        Ok(())
    }

    /// The error a stopped call reports: a cancel wins over a deadline,
    /// because the user asked for it.
    pub fn stop_reason(&self) -> DriverError {
        if self.is_cancelled() {
            DriverError::cancelled()
        } else {
            DriverError::timeout()
        }
    }

    /// Runs `future` until it ends, the token fires or the deadline passes.
    pub async fn run<T>(&self, future: impl Future<Output = Result<T, DriverError>>) -> Result<T, DriverError> {
        let deadline = async {
            match self.deadline {
                Some(deadline) => tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)).await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            biased;
            () = self.cancel.cancelled() => Err(DriverError::cancelled()),
            () = deadline => Err(DriverError::timeout()),
            result = future => result,
        }
    }

    /// Waits until the call should stop. A driver spawns this beside a
    /// blocking statement and interrupts the engine when it returns.
    pub async fn stopped(&self) {
        let deadline = async {
            match self.deadline {
                Some(deadline) => tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)).await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            () = self.cancel.cancelled() => {},
            () = deadline => {},
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ErrorCode;

    #[tokio::test]
    async fn run_returns_the_result_when_nothing_stops_it() {
        let options = CallOptions::default();
        let result = options.run(async { Ok::<_, DriverError>(7) }).await;
        assert_eq!(result, Ok(7));
    }

    #[tokio::test]
    async fn run_stops_on_cancel() {
        let options = CallOptions::default();
        options.cancel.cancel();
        let result = options.run(std::future::pending::<Result<(), DriverError>>()).await;
        assert_eq!(result.map_err(|e| e.code), Err(ErrorCode::Cancelled));
    }

    #[tokio::test]
    async fn run_stops_at_the_deadline() {
        let options = CallOptions::default().with_timeout(Duration::from_millis(10));
        let result = options.run(std::future::pending::<Result<(), DriverError>>()).await;
        assert_eq!(result.map_err(|e| e.code), Err(ErrorCode::Timeout));
    }

    #[test]
    fn a_child_is_cancelled_with_its_parent_but_not_the_reverse() {
        let parent = CallOptions::default();
        let child = parent.child();
        child.cancel.cancel();
        assert!(!parent.is_cancelled());
        let other = parent.child();
        parent.cancel.cancel();
        assert!(other.is_cancelled());
    }
}

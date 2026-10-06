use std::fmt;

/// Redacted wrapper to ensure sensitive data (addresses, keys, memos) is never accidentally printed.
#[derive(Clone, PartialEq, Eq)]
pub struct Redacted<T>(pub T);

impl<T> Redacted<T> {
    pub fn new(val: T) -> Self {
        Self(val)
    }

    pub fn into_inner(self) -> T {
        self.0
    }

    pub fn inner(&self) -> &T {
        &self.0
    }
}

impl<T> fmt::Debug for Redacted<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[REDACTED]")
    }
}

impl<T> fmt::Display for Redacted<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[REDACTED]")
    }
}

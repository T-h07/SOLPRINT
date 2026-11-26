//! Secret wrapper.
//!
//! `Secret<T>` has no `Display`, no `Serialize` and a redacting `Debug`, so a
//! secret cannot reach a log line, an API response or a panic message by
//! accident. Reading the value requires an explicit, greppable
//! [`Secret::expose`] call at the point of use.

use std::fmt;

pub struct Secret<T>(T);

impl<T> Secret<T> {
    pub const fn new(value: T) -> Self {
        Self(value)
    }

    /// Access the secret value. Every call site is a point where the secret
    /// leaves the wrapper; keep them few and never pass the result to a
    /// logging or formatting macro.
    pub const fn expose(&self) -> &T {
        &self.0
    }
}

impl<T: Clone> Clone for Secret<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T> fmt::Debug for Secret<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret([REDACTED])")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_never_reveals_value() {
        let s = Secret::new("sk_synthetic_do_not_log_123".to_owned());
        let rendered = format!("{s:?} {:#?}", s);
        assert!(!rendered.contains("sk_synthetic"));
        assert!(rendered.contains("REDACTED"));
        assert_eq!(s.expose(), "sk_synthetic_do_not_log_123");
    }
}

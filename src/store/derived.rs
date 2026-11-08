//! A derivation known now, or reached through outputs at build time.
use std::fmt;
use std::ops::Deref;

use super::path::StorePath;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct DerivedPath {
    pub root: StorePath,
    /// Output names in producer-to-consumer order.
    pub chain: Vec<String>,
}

impl From<StorePath> for DerivedPath {
    fn from(root: StorePath) -> Self {
        Self {
            root,
            chain: Vec::new(),
        }
    }
}

impl Deref for DerivedPath {
    type Target = StorePath;
    fn deref(&self) -> &StorePath {
        &self.root
    }
}

impl fmt::Display for DerivedPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.root)?;
        for output in &self.chain {
            write!(f, "!{output}")?;
        }
        Ok(())
    }
}

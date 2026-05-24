//! Step-budget newtype.  Every evaluator function spends one unit of fuel
//! per call; when fuel runs out, evaluation aborts with
//! [`Error::FuelExhausted`].
//!
//! [`Error::FuelExhausted`]: crate::error::Error::FuelExhausted

use crate::error::Error;

/// A fuel budget.  Construct with [`Fuel::new`]; spend with [`Fuel::spend`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fuel {
    limit: u64,
    remaining: u64,
}

impl Fuel {
    /// A fresh budget of `limit` steps.
    #[must_use]
    pub fn new(limit: u64) -> Self {
        Self {
            limit,
            remaining: limit,
        }
    }

    /// The original budget.
    #[must_use]
    pub fn limit(&self) -> u64 {
        self.limit
    }

    /// Steps still available.
    #[must_use]
    pub fn remaining(&self) -> u64 {
        self.remaining
    }

    /// Spend one unit; returns [`Error::FuelExhausted`] when exhausted.
    ///
    /// # Errors
    ///
    /// [`Error::FuelExhausted`] when there is no remaining fuel.
    pub fn spend(self) -> Result<Self, Error> {
        match self.remaining {
            0 => Err(Error::FuelExhausted { limit: self.limit }),
            n => Ok(Self {
                limit: self.limit,
                remaining: n - 1,
            }),
        }
    }
}

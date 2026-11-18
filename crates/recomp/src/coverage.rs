//! Aggregate coverage: executed functions served by the library versus the interpreter.
//! Only counts are kept (study gate 1); no addresses, names or tables.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Coverage {
    pub executed: u64,
    pub library: u64,
}

impl Coverage {
    /// Executed functions served by the interpreter.
    pub fn interpreter(&self) -> u64 {
        self.executed.saturating_sub(self.library)
    }

    /// Fraction served by the library, as parts per 10 000 (no floats); 0 when nothing ran.
    pub fn library_basis_points(&self) -> u64 {
        match self.executed {
            0 => 0,
            n => self.library.min(n) * 10_000 / n,
        }
    }

    /// One line of aggregates for an issue comment.
    pub fn summary(&self) -> String {
        let bp = self.library_basis_points();
        format!(
            "executed={} library={} interpreter={} library_share={}.{:02}%",
            self.executed,
            self.library,
            self.interpreter(),
            bp / 100,
            bp % 100
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shares() {
        let c = Coverage {
            executed: 8,
            library: 6,
        };
        assert_eq!(c.interpreter(), 2);
        assert_eq!(c.library_basis_points(), 7500);
        assert_eq!(
            c.summary(),
            "executed=8 library=6 interpreter=2 library_share=75.00%"
        );
        assert_eq!(Coverage::default().library_basis_points(), 0);
    }
}

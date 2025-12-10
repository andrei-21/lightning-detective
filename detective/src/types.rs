use std::{fmt, ops::Rem};
use thousands::Separable;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Sat(pub u64);

impl fmt::Display for Sat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sats = self.0;
        write!(f, "{}", sats.separate_with_commas())?;
        if !f.alternate() {
            write!(f, " sat")?;
            if sats != 1 {
                write!(f, "s")?;
            }
        }
        Ok(())
    }
}

impl From<bitcoin::Amount> for Sat {
    fn from(amount: bitcoin::Amount) -> Self {
        Self(amount.to_sat())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Msat(pub u64);

impl fmt::Display for Msat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            1_000 => write!(f, "1")?,
            msat if msat.is_multiple_of(1_000) => {
                write!(f, "{}", (msat / 1_000).separate_with_commas())?
            }
            msat => {
                let sats = (msat / 1_000).separate_with_commas();
                let remainder = msat.rem(1_000);
                write!(f, "{sats}.{remainder:03}")?
            }
        };
        if !f.alternate() {
            write!(f, " sat")?;
            if self.0 != 1_000 {
                write!(f, "s")?;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
pub enum MsatRange {
    Any,
    Between(Msat, Msat),
    Min(Msat),
    Max(Msat),
}

impl fmt::Display for MsatRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Any => write!(f, "any"),
            Self::Between(min, max) if min == max => min.fmt(f),
            Self::Between(min, max) => {
                write!(f, "{min:#}–")?;
                max.fmt(f)
            }
            Self::Min(min) => {
                write!(f, "≥ ")?;
                min.fmt(f)
            }
            Self::Max(max) => {
                write!(f, "≤ ")?;
                max.fmt(f)
            }
        }
    }
}

impl MsatRange {
    pub fn min(&self) -> Msat {
        match self {
            Self::Any => Msat(0),
            Self::Between(min, _max) => *min,
            Self::Min(min) => *min,
            Self::Max(_max) => Msat(0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Msat, MsatRange, Sat};

    #[test]
    fn sat_display() {
        assert_eq!(Sat(0).to_string(), "0 sats");
        assert_eq!(Sat(1).to_string(), "1 sat");
        assert_eq!(Sat(2).to_string(), "2 sats");
        assert_eq!(Sat(1_000).to_string(), "1,000 sats");

        assert_eq!(format!("{:#}", Sat(2)), "2");
        assert_eq!(format!("{:#}", Sat(1_000)), "1,000");
    }

    #[test]
    fn sat_from_bitcoin_amount() {
        let amount = bitcoin::Amount::from_sat(42);
        let sat: Sat = amount.into();
        assert_eq!(sat, Sat(42));
    }

    #[test]
    fn msat_display() {
        assert_eq!(Msat(1_000).to_string(), "1 sat");
        assert_eq!(Msat(2_000).to_string(), "2 sats");
        assert_eq!(Msat(1_000_000).to_string(), "1,000 sats");
        assert_eq!(Msat(12_345).to_string(), "12.345 sats");
        assert_eq!(Msat(1).to_string(), "0.001 sats");

        assert_eq!(format!("{:#}", Msat(2_000)), "2");
        assert_eq!(format!("{:#}", Msat(12_345)), "12.345");
    }

    #[test]
    fn msat_range_display() {
        let range = MsatRange::Between(Msat(500), Msat(1_000));
        assert_eq!(range.to_string(), "0.500–1 sat");
        assert_eq!(format!("{range:#}"), "0.500–1");

        let range = MsatRange::Between(Msat(1_000), Msat(2_000));
        assert_eq!(range.to_string(), "1–2 sats");
        assert_eq!(format!("{range:#}"), "1–2");

        let range = MsatRange::Between(Msat(1_000), Msat(2_345));
        assert_eq!(range.to_string(), "1–2.345 sats");
        assert_eq!(format!("{range:#}"), "1–2.345");

        let range = MsatRange::Min(Msat(1_000));
        assert_eq!(range.to_string(), "≥ 1 sat");
        assert_eq!(format!("{range:#}"), "≥ 1");

        let range = MsatRange::Max(Msat(2_000));
        assert_eq!(range.to_string(), "≤ 2 sats");
        assert_eq!(format!("{range:#}"), "≤ 2");

        assert_eq!(MsatRange::Any.to_string(), "any");
        assert_eq!(format!("{:#}", MsatRange::Any), "any");
    }
}

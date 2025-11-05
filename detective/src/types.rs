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
pub struct MsatRange(pub Msat, pub Msat);

impl fmt::Display for MsatRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0 == self.1 {
            self.0.fmt(f)
        } else {
            write!(f, "{:#}-", self.0)?;
            self.1.fmt(f)
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
        let range = MsatRange(Msat(1_000), Msat(1_000));
        assert_eq!(range.to_string(), "1 sat");
        assert_eq!(format!("{range:#}"), "1");

        let range = MsatRange(Msat(2_000), Msat(2_000));
        assert_eq!(range.to_string(), "2 sats");
        assert_eq!(format!("{range:#}"), "2");

        let range = MsatRange(Msat(1_000), Msat(2_345));
        assert_eq!(range.to_string(), "1-2.345 sats");
        assert_eq!(format!("{range:#}"), "1-2.345");
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvestigateValueKind {
    Bolt12Invoice,
    Bolt12StaticInvoice,
}

impl InvestigateValueKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Bolt12Invoice => "bolt12_invoice",
            Self::Bolt12StaticInvoice => "bolt12_static_invoice",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "bolt12_invoice" => Some(Self::Bolt12Invoice),
            "bolt12_static_invoice" => Some(Self::Bolt12StaticInvoice),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvestigateValue {
    pub kind: InvestigateValueKind,
    pub payload: String,
}

impl InvestigateValue {
    pub fn new(kind: InvestigateValueKind, payload: String) -> Self {
        Self { kind, payload }
    }

    pub fn parse(input: &str) -> Option<Self> {
        let (kind, payload) = input.split_once(':')?;
        let kind = InvestigateValueKind::parse(kind)?;
        if payload.is_empty() {
            return None;
        }
        Some(Self::new(kind, payload.to_string()))
    }

    pub fn as_encoded(&self) -> String {
        format!("{}:{}", self.kind.as_str(), self.payload)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_encoded_investigate_value() {
        let value = InvestigateValue::parse("bolt12_invoice:012345").unwrap();
        assert_eq!(value.kind, InvestigateValueKind::Bolt12Invoice);
        assert_eq!(value.payload, "012345");
    }

    #[test]
    fn rejects_unknown_kinds() {
        let value = InvestigateValue::parse("invoice:012345");
        assert!(value.is_none());
    }
}

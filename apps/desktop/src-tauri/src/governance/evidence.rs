#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum EvidenceLevel {
    Unavailable,
    Inferred,
    Derived,
    Observed,
    UserConfirmed,
}

impl EvidenceLevel {
    pub fn parse(value: &str) -> Self {
        match value {
            "observed" => Self::Observed,
            "derived" => Self::Derived,
            "inferred" | "estimated" => Self::Inferred,
            "user-confirmed" => Self::UserConfirmed,
            _ => Self::Unavailable,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Observed => "observed",
            Self::Derived => "derived",
            Self::Inferred => "inferred",
            Self::UserConfirmed => "user-confirmed",
            Self::Unavailable => "unavailable",
        }
    }

    pub fn confidence(self) -> f64 {
        match self {
            Self::Observed => 0.98,
            Self::Derived => 0.82,
            Self::Inferred => 0.45,
            Self::UserConfirmed => 1.0,
            Self::Unavailable => 0.0,
        }
    }
}

pub fn exact_coverage(value: &str) -> bool {
    value.starts_with("exact-") || value == "exact"
}

pub fn merge_coverage<'a>(values: impl Iterator<Item = &'a str>) -> String {
    let mut best = "not-recorded";
    let mut best_rank = 0;
    for value in values {
        let rank = if exact_coverage(value) {
            4
        } else if value.starts_with("derived-") || value == "derived" {
            3
        } else if value.starts_with("partial-") || value == "partial" {
            2
        } else if value != "not-recorded" && value != "unavailable" {
            1
        } else {
            0
        };
        if rank > best_rank || (rank == best_rank && value < best) {
            best = value;
            best_rank = rank;
        }
    }
    best.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evidence_levels_never_promote_inferred_to_observed() {
        assert!(EvidenceLevel::Observed > EvidenceLevel::Inferred);
        assert_eq!(EvidenceLevel::parse("estimated"), EvidenceLevel::Inferred);
        assert_eq!(
            EvidenceLevel::parse("not-recorded"),
            EvidenceLevel::Unavailable
        );
    }

    #[test]
    fn exact_coverage_wins_a_mixed_projection() {
        assert_eq!(
            merge_coverage(["partial-delegation", "exact-delegation"].into_iter()),
            "exact-delegation"
        );
    }
}

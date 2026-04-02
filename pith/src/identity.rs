/// Identity and group model for the 0-bytes permission system.
///
/// Identities are numbered slots (unbounded). The privilege tier is
/// determined by the first digit of the identity number AS WRITTEN
/// in the filesystem (preserving leading zeros):
///   0xx=omni, 1xx=shadow, 2xx=superroot, 3xx=root, 4xx=admin,
///   5xx=permissioned, 6xx=user, 7xx=shared, 8xx=guest, 9xx=digitalconsciousness

/// Privilege tiers derived from the first digit of the identity number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PrivilegeTier {
    Omni,                // 0xx — omniscient, above the system
    Shadow,              // 1xx — invisible system processes
    Superroot,           // 2xx — can modify hard/ (the ROM)
    Root,                // 3xx — full system administration
    Admin,               // 4xx — manage identities and groups
    Permissioned,        // 5xx — elevated custom permissions
    User,                // 6xx — standard access
    Shared,              // 7xx — multi-tenant shared access
    Guest,               // 8xx — minimal read access
    DigitalConsciousness, // 9xx — AI/autonomous agent
}

impl PrivilegeTier {
    /// Derive the tier from the first character of the identity's filesystem name.
    ///
    /// This preserves leading zeros: "001" → first char '0' → Omni.
    /// For numeric IDs without the original string, use `from_id`.
    pub fn from_name(name: &str) -> Self {
        match name.chars().next() {
            Some('0') => Self::Omni,
            Some('1') => Self::Shadow,
            Some('2') => Self::Superroot,
            Some('3') => Self::Root,
            Some('4') => Self::Admin,
            Some('5') => Self::Permissioned,
            Some('6') => Self::User,
            Some('7') => Self::Shared,
            Some('8') => Self::Guest,
            Some('9') => Self::DigitalConsciousness,
            _ => Self::Guest, // fallback for non-numeric names
        }
    }

    /// Derive the tier from a numeric identity ID.
    ///
    /// For IDs < 100, this extracts the first digit correctly.
    /// For IDs >= 100, uses the leading digit.
    pub fn from_id(id: u64) -> Self {
        let mut n = id;
        while n >= 10 {
            n /= 10;
        }
        match n {
            0 => Self::Omni,
            1 => Self::Shadow,
            2 => Self::Superroot,
            3 => Self::Root,
            4 => Self::Admin,
            5 => Self::Permissioned,
            6 => Self::User,
            7 => Self::Shared,
            8 => Self::Guest,
            9 => Self::DigitalConsciousness,
            _ => Self::Guest, // fallback
        }
    }
}

/// An identity in the 0-bytes OS.
#[derive(Debug, Clone)]
pub struct Identity {
    /// Numeric identifier (unbounded).
    pub id: u64,

    /// The original filesystem name (preserves leading zeros, e.g., "001").
    pub name: String,

    /// Privilege tier (derived from first digit of the name).
    pub tier: PrivilegeTier,

    /// Group memberships (names of groups this identity belongs to).
    pub groups: Vec<String>,
}

impl Identity {
    /// Create a new identity from its filesystem name.
    ///
    /// The name is the directory name (e.g., "001", "600", "42000").
    /// The tier is derived from the first character of the name.
    pub fn from_name(name: &str, id: u64) -> Self {
        Self {
            id,
            name: name.to_string(),
            tier: PrivilegeTier::from_name(name),
            groups: Vec::new(),
        }
    }

    /// Create a new identity from just a numeric ID (no original name).
    pub fn new(id: u64) -> Self {
        Self {
            id,
            name: format!("{}", id),
            tier: PrivilegeTier::from_id(id),
            groups: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tier_from_name() {
        // Leading zeros preserved.
        assert_eq!(PrivilegeTier::from_name("001"), PrivilegeTier::Omni);
        assert_eq!(PrivilegeTier::from_name("042"), PrivilegeTier::Omni);
        assert_eq!(PrivilegeTier::from_name("099"), PrivilegeTier::Omni);
        assert_eq!(PrivilegeTier::from_name("100"), PrivilegeTier::Shadow);
        assert_eq!(PrivilegeTier::from_name("200"), PrivilegeTier::Superroot);
        assert_eq!(PrivilegeTier::from_name("300"), PrivilegeTier::Root);
        assert_eq!(PrivilegeTier::from_name("420"), PrivilegeTier::Admin);
        assert_eq!(PrivilegeTier::from_name("500"), PrivilegeTier::Permissioned);
        assert_eq!(PrivilegeTier::from_name("600"), PrivilegeTier::User);
        assert_eq!(PrivilegeTier::from_name("777"), PrivilegeTier::Shared);
        assert_eq!(PrivilegeTier::from_name("800"), PrivilegeTier::Guest);
        assert_eq!(PrivilegeTier::from_name("999"), PrivilegeTier::DigitalConsciousness);
    }

    #[test]
    fn test_tier_from_id() {
        assert_eq!(PrivilegeTier::from_id(0), PrivilegeTier::Omni);
        assert_eq!(PrivilegeTier::from_id(1), PrivilegeTier::Shadow);
        assert_eq!(PrivilegeTier::from_id(42), PrivilegeTier::Admin);
        assert_eq!(PrivilegeTier::from_id(100), PrivilegeTier::Shadow);
        assert_eq!(PrivilegeTier::from_id(600), PrivilegeTier::User);
        assert_eq!(PrivilegeTier::from_id(777), PrivilegeTier::Shared);
        assert_eq!(PrivilegeTier::from_id(999), PrivilegeTier::DigitalConsciousness);
        assert_eq!(PrivilegeTier::from_id(1337), PrivilegeTier::Shadow);
        assert_eq!(PrivilegeTier::from_id(42000), PrivilegeTier::Admin);
    }

    #[test]
    fn test_identity_from_name() {
        let id = Identity::from_name("001", 1);
        assert_eq!(id.id, 1);
        assert_eq!(id.name, "001");
        assert_eq!(id.tier, PrivilegeTier::Omni);

        let id = Identity::from_name("600", 600);
        assert_eq!(id.tier, PrivilegeTier::User);
    }
}

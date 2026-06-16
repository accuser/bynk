//! Centralised string literals for the language's built-in vocabulary (refactor
//! track item 8, v0.29.11). Built-in type/method names were compared as bare
//! string literals scattered across the checker, emitter, and project modules;
//! a typo was a silent never-match. One edit point per name now.

/// Built-in type names (as they appear in source / qualified positions).
pub mod types {
    pub const JSON: &str = "Json";
    pub const LIST: &str = "List";
    pub const MAP: &str = "Map";
    pub const INT: &str = "Int";
    pub const FLOAT: &str = "Float";
    pub const HTTP_RESULT: &str = "HttpResult";
    pub const QUEUE_RESULT: &str = "QueueResult";
}

/// Privileged built-in member names — constructors (`of`/`unsafe`), the refined
/// raw accessor (`raw`), and the effect fold (`foldEff`).
pub mod methods {
    pub const OF: &str = "of";
    pub const UNSAFE: &str = "unsafe";
    pub const RAW: &str = "raw";
    pub const FOLD_EFF: &str = "foldEff";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_hold_expected_values() {
        assert_eq!(types::JSON, "Json");
        assert_eq!(types::HTTP_RESULT, "HttpResult");
        assert_eq!(methods::OF, "of");
        assert_eq!(methods::FOLD_EFF, "foldEff");
    }
}

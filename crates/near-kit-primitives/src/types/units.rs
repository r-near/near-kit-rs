//! NEAR token amount and gas unit types, re-exported from upstream crates.
//!
//! Both parse from strings with their upstream `FromStr` impls
//! (`"5 NEAR".parse::<NearToken>()`, `"30 Tgas".parse::<Gas>()`); methods
//! take them typed.

pub use near_gas::NearGas as Gas;
pub use near_token::NearToken;

#[cfg(test)]
mod tests {
    use super::*;

    /// The string forms the removed `IntoNearToken`/`IntoGas` conversions
    /// accepted, as far as upstream `FromStr` covers them.
    #[test]
    fn upstream_from_str_parses_unit_strings() {
        let near = |s: &str| s.parse::<NearToken>();
        assert_eq!(near("5 NEAR").unwrap(), NearToken::from_near(5));
        assert_eq!(near("1.5 NEAR").unwrap(), NearToken::from_millinear(1500));
        assert_eq!(
            near("100 milliNEAR").unwrap(),
            NearToken::from_millinear(100)
        );
        assert_eq!(
            near("1000 yoctoNEAR").unwrap(),
            NearToken::from_yoctonear(1000)
        );
        assert!(near("123").is_err(), "a bare number has no unit");

        let gas = |s: &str| s.parse::<Gas>();
        assert_eq!(gas("30 Tgas").unwrap(), Gas::from_tgas(30));
        assert_eq!(gas("5 Ggas").unwrap(), Gas::from_ggas(5));
        assert_eq!(gas("1.5 Tgas").unwrap(), Gas::from_ggas(1500));
        assert!(gas("thirty").is_err());
    }
}

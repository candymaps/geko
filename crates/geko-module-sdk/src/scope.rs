use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize};

use crate::country::{CountryCode, RegionCode};
use crate::{GekoError, Result};

/// Countries and regions to download, build and serve.
/// A country with no regions means the whole country.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Scope {
    countries: BTreeMap<CountryCode, Vec<RegionCode>>,
}

impl Scope {
    pub fn add(&mut self, country: CountryCode, regions: Vec<RegionCode>) -> Result<()> {
        if let Some(r) = regions.iter().find(|r| r.country() != country) {
            return Err(GekoError::Config(format!("region {r} does not belong to country {country}")));
        }
        if self.countries.contains_key(&country) {
            return Err(GekoError::Config(format!("country {country} appears more than once in the scope")));
        }
        let mut regions = regions;
        regions.sort();
        regions.dedup();
        self.countries.insert(country, regions);
        Ok(())
    }

    pub fn is_empty(&self) -> bool {
        self.countries.is_empty()
    }

    pub fn countries(&self) -> impl Iterator<Item = CountryCode> + '_ {
        self.countries.keys().copied()
    }

    pub fn partitions(&self) -> Vec<Partition> {
        self.countries
            .iter()
            .flat_map(|(country, regions)| {
                if regions.is_empty() {
                    vec![Partition { country: *country, region: None }]
                } else {
                    regions.iter().map(|r| Partition { country: *country, region: Some(r.clone()) }).collect()
                }
            })
            .collect()
    }

    /// Countries of this scope that a provider with the given coverage serves.
    pub fn covered_by(&self, coverage: &Coverage) -> Vec<CountryCode> {
        self.countries().filter(|c| coverage.covers(*c)).collect()
    }
}

/// Checked as `Scope::add` checks it: a module is only handed a scope the host could build.
impl<'de> Deserialize<'de> for Scope {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Raw {
            countries: BTreeMap<CountryCode, Vec<RegionCode>>,
        }
        let mut scope = Scope::default();
        for (country, regions) in Raw::deserialize(d)?.countries {
            scope.add(country, regions).map_err(serde::de::Error::custom)?;
        }
        Ok(scope)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Partition {
    pub country: CountryCode,
    pub region: Option<RegionCode>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Coverage {
    Global,
    Countries(Vec<CountryCode>),
}

impl Coverage {
    pub fn covers(&self, country: CountryCode) -> bool {
        match self {
            Coverage::Global => true,
            Coverage::Countries(list) => list.contains(&country),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cc(s: &str) -> CountryCode {
        CountryCode::new(s).unwrap()
    }

    fn rc(s: &str) -> RegionCode {
        RegionCode::new(s).unwrap()
    }

    #[test]
    fn partitions_per_region_or_whole_country() {
        let mut scope = Scope::default();
        scope.add(cc("ES"), vec![rc("ES-M"), rc("ES-B"), rc("ES-B")]).unwrap();
        scope.add(cc("PT"), vec![]).unwrap();
        assert_eq!(
            scope.partitions(),
            vec![
                Partition { country: cc("ES"), region: Some(rc("ES-B")) },
                Partition { country: cc("ES"), region: Some(rc("ES-M")) },
                Partition { country: cc("PT"), region: None },
            ]
        );
    }

    #[test]
    fn rejects_inconsistent_scope() {
        let mut scope = Scope::default();
        assert!(scope.add(cc("ES"), vec![rc("FR-75")]).is_err());
        scope.add(cc("ES"), vec![]).unwrap();
        assert!(scope.add(cc("ES"), vec![]).is_err());
    }

    #[test]
    fn a_scope_crosses_a_boundary_unchanged() {
        let mut scope = Scope::default();
        scope.add(cc("ES"), vec![rc("ES-PM")]).unwrap();
        scope.add(cc("LU"), vec![]).unwrap();
        let text = serde_json::to_string(&scope).unwrap();
        assert_eq!(serde_json::from_str::<Scope>(&text).unwrap(), scope);
        assert!(serde_json::from_str::<Scope>(r#"{"countries":{"ES":["FR-75"]}}"#).is_err());
    }

    #[test]
    fn national_provider_only_covers_its_countries() {
        let mut scope = Scope::default();
        scope.add(cc("ES"), vec![]).unwrap();
        scope.add(cc("PT"), vec![]).unwrap();
        assert_eq!(scope.covered_by(&Coverage::Countries(vec![cc("ES")])), vec![cc("ES")]);
        assert_eq!(scope.covered_by(&Coverage::Global), vec![cc("ES"), cc("PT")]);
        assert!(scope.covered_by(&Coverage::Countries(vec![cc("FR")])).is_empty());
    }
}

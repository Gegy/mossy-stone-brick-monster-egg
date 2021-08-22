use regex::Regex;

use crate::Config;

pub struct NameFilter {
    regex: Vec<Regex>,
}

impl NameFilter {
    pub fn new(config: &Config) -> NameFilter {
        NameFilter {
            regex: config.ban_regex.iter()
                .map(|regex| Regex::new(regex).unwrap())
                .collect()
        }
    }

    #[inline]
    pub fn is_illegal(&self, name: &str) -> bool {
        self.regex.iter().any(|regex| regex.is_match(name))
    }
}

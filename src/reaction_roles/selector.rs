use std::collections::HashMap;
use std::str::FromStr;

use regex::Regex;
use serde::{Deserialize, Serialize};
use serenity::model::prelude::*;

#[derive(Serialize, Deserialize, Default)]
pub struct Selector(HashMap<Emoji, RoleId>);

impl Selector {
    pub fn new() -> Self {
        Selector(HashMap::new())
    }

    #[inline]
    pub fn insert_role(&mut self, emoji: Emoji, role: RoleId) {
        self.0.insert(emoji, role);
    }

    #[inline]
    pub fn get_role(&self, emoji: &Emoji) -> Option<RoleId> {
        self.0.get(emoji).copied()
    }

    #[inline]
    pub fn contains(&self, emoji: &Emoji) -> bool {
        self.0.contains_key(emoji)
    }

    #[inline]
    pub fn iter(&self) -> impl Iterator<Item=(&Emoji, &RoleId)> {
        self.0.iter()
    }
}

impl Selector {
    pub fn parse(content: &str) -> Selector {
        let role_pattern = Regex::new(r#"<@&([^>]*)>"#).unwrap();
        let custom_emoji_pattern = Regex::new(r#"<:([^>]*>)"#).unwrap();
        let unicode_emoji_pattern = Regex::new(r#"[\p{Emoji}--\p{Digit}]"#).unwrap();

        let mut selector = Selector::new();

        for line in content.lines() {
            let mut roles = role_pattern.find_iter(line)
                .filter_map(|role| {
                    let role = role.as_str();
                    serenity::utils::parse_role(role)
                })
                .map(|role| RoleId(role));

            let custom_emoji = custom_emoji_pattern.find_iter(line)
                .filter_map(|custom_emoji| {
                    let custom_emoji = custom_emoji.as_str();
                    serenity::utils::parse_emoji(custom_emoji)
                })
                .map(|custom_emoji| {
                    Emoji::from(ReactionType::Custom {
                        animated: false,
                        id: custom_emoji.id,
                        name: Some(custom_emoji.name),
                    })
                });

            let unicode_emoji = unicode_emoji_pattern.find_iter(line)
                .map(|unicode_emoji| {
                    let unicode_emoji = unicode_emoji.as_str().to_owned();
                    Emoji::from(ReactionType::Unicode(unicode_emoji))
                });

            let mut emoji = custom_emoji.chain(unicode_emoji);

            if let (Some(role), Some(emoji)) = (roles.next(), emoji.next()) {
                selector.insert_role(emoji, role);
            }
        }

        selector
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct Emoji(String);

impl From<ReactionType> for Emoji {
    fn from(reaction: ReactionType) -> Self {
        match reaction {
            ReactionType::Custom {
                animated: _,
                id,
                name
            } => match name {
                Some(name) => Emoji(format!("<:{}:{}>", name, id)),
                None => Emoji(format!("<:{}>", id)),
            },
            ReactionType::Unicode(unicode) => Emoji(unicode),
            _ => panic!("unknown reaction type")
        }
    }
}

impl Into<ReactionType> for Emoji {
    fn into(self) -> ReactionType {
        match EmojiIdentifier::from_str(&self.0) {
            Ok(custom) => {
                ReactionType::Custom {
                    animated: false,
                    id: custom.id,
                    name: Some(custom.name),
                }
            }
            Err(_) => ReactionType::Unicode(self.0),
        }
    }
}

impl FromStr for Emoji {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, ()> {
        Ok(Emoji(s.to_owned()))
    }
}

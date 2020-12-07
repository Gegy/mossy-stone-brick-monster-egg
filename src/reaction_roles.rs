use std::collections::HashMap;
use std::str::FromStr;

use regex::Regex;
use serde::{Deserialize, Serialize};
use serenity::framework::standard::{
    Args, CommandResult,
    macros::command,
};
use serenity::model::prelude::*;
use serenity::prelude::*;

use super::Config;

pub struct ReactionRoleKey;

impl TypeMapKey for ReactionRoleKey {
    type Value = Config<Messages>;
}

#[derive(Serialize, Deserialize, Default)]
pub struct Messages(HashMap<MessageId, Group>);

impl Messages {
    #[inline]
    pub fn get_or_create_group(&mut self, message: MessageId) -> &mut Group {
        self.0.entry(message).or_insert_with(|| Group::new())
    }

    #[inline]
    pub fn get_group(&self, message: MessageId) -> Option<&Group> {
        self.0.get(&message)
    }

    #[inline]
    pub fn get_group_mut(&mut self, message: MessageId) -> Option<&mut Group> {
        self.0.get_mut(&message)
    }

    #[inline]
    pub fn contains_group(&self, message: MessageId) -> bool {
        self.0.contains_key(&message)
    }

    #[inline]
    pub fn remove_group(&mut self, message: MessageId) -> Option<Group> {
        self.0.remove(&message)
    }
}

#[derive(Serialize, Deserialize, Default)]
pub struct Group(HashMap<Emoji, RoleId>);

impl Group {
    pub fn new() -> Self {
        Group(HashMap::new())
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
    pub fn clear(&mut self) {
        self.0.clear();
    }

    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = (&Emoji, &RoleId)> {
        self.0.iter()
    }
}

pub async fn add_reaction(ctx: Context, reaction: Reaction) -> serenity::Result<()> {
    let (guild, user) = match (reaction.guild_id, reaction.user_id) {
        (Some(guild), Some(user)) => (guild, user),
        _ => return Ok(()),
    };

    let data = ctx.data.read().await;
    let messages = data.get::<ReactionRoleKey>().unwrap();

    if let Some(group) = messages.get_group(reaction.message_id) {
        let emoji = reaction.emoji.clone().into();
        match group.get_role(&emoji) {
            Some(role) => {
                let mut member: Member = guild.member(&ctx, user).await?;
                if !member.user.bot {
                    member.add_role(&ctx.http, role).await?;
                }
            }
            None => reaction.delete(&ctx.http).await?,
        }
    }

    Ok(())
}

pub async fn remove_reaction(ctx: &Context, reaction: Reaction) -> serenity::Result<()> {
    let (guild, user) = match (reaction.guild_id, reaction.user_id) {
        (Some(guild), Some(user)) => (guild, user),
        _ => return Ok(()),
    };

    let data = ctx.data.read().await;
    let messages = data.get::<ReactionRoleKey>().unwrap();

    if let Some(group) = messages.get_group(reaction.message_id) {
        let emoji = reaction.emoji.clone().into();
        if let Some(role) = group.get_role(&emoji) {
            let mut member: Member = guild.member(ctx, user).await?;
            member.remove_role(&ctx.http, role).await?;
        }
    }

    Ok(())
}

async fn is_message_registered(ctx: &Context, message: MessageId) -> bool {
    let data = ctx.data.read().await;
    let messages = data.get::<ReactionRoleKey>().unwrap();

    messages.contains_group(message)
}

pub async fn delete_message(ctx: Context, message: MessageId) {
    if !is_message_registered(&ctx, message).await {
        return;
    }

    let mut data = ctx.data.write().await;
    let messages = data.get_mut::<ReactionRoleKey>().unwrap();

    messages.write(|messages| {
        messages.remove_group(message);
    }).await;
}

pub async fn update_message(mut ctx: Context, channel: ChannelId, message: MessageId, content: Option<String>) {
    if let Some(content) = content {
        if !is_message_registered(&ctx, message).await {
            return;
        }

        {
            let mut data = ctx.data.write().await;
            let messages = data.get_mut::<ReactionRoleKey>().unwrap();

            messages.write(|messages| {
                if let Some(group) = messages.get_group_mut(message) {
                    group.clear();
                    for (emoji, role) in parse_group(&content) {
                        group.insert_role(emoji, role);
                    }
                }
            }).await;
        }

        apply_group_reactions(&mut ctx, channel, message).await;
    }
}

async fn apply_group_reactions(ctx: &Context, channel: ChannelId, message: MessageId) {
    let data = ctx.data.read().await;
    let messages = data.get::<ReactionRoleKey>().unwrap();

    if let Some(group) = messages.get_group(message) {
        if let Ok(target_message) = channel.message(&ctx.http, message).await {
            let current_user = ctx.cache.current_user_id().await;

            let own_reactions = target_message.reactions.iter()
                .filter(|reaction| reaction.me);

            for reaction in own_reactions {
                let _ = ctx.http.delete_reaction(channel.0, message.0, Some(current_user.0), &reaction.reaction_type).await;
            }

            for (emoji, _) in group.iter() {
                let _ = target_message.react(ctx, emoji.clone()).await;
            }
        }
    }
}

fn parse_group(content: &str) -> Vec<(Emoji, RoleId)> {
    let role_pattern = Regex::new(r#"<@&([^>]*)>"#).unwrap();
    let custom_emoji_pattern = Regex::new(r#"<:([^>]*>)"#).unwrap();
    let unicode_emoji_pattern = Regex::new(r#"[\p{Emoji}--\p{Digit}]"#).unwrap();

    let mut group = Vec::new();

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
            group.push((emoji, role));
        }
    }

    group
}

#[command]
pub async fn track_reactions(ctx: &Context, msg: &Message, mut args: Args) -> CommandResult {
    // TODO: error handling
    let message_id = MessageId(args.single::<u64>().unwrap());

    let _ = msg.delete(ctx).await;

    if let Ok(target_message) = msg.channel_id.message(&ctx.http, message_id).await {
        {
            let mut data = ctx.data.write().await;
            let messages = data.get_mut::<ReactionRoleKey>().unwrap();
            messages.write(|messages| {
                let group = messages.get_or_create_group(message_id);
                for (emoji, role) in parse_group(&target_message.content) {
                    group.insert_role(emoji, role);
                }
            }).await;
        }

        apply_group_reactions(ctx, msg.channel_id, message_id).await;
    } else {
        let _ = msg.reply(ctx, "failed to find message with that id! make sure it's in this channel").await;
    }

    Ok(())
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

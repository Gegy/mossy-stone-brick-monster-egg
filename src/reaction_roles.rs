use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serenity::model::prelude::*;
use serenity::prelude::*;

use selector::*;

use super::{CommandError, CommandResult, Persistent};

mod selector;

pub struct StateKey;

impl TypeMapKey for StateKey {
    type Value = Persistent<State>;
}

#[derive(Serialize, Deserialize, Default)]
pub struct State(HashMap<MessageId, Selector>);

impl State {
    #[inline]
    pub fn insert_selector(&mut self, message: MessageId, selector: Selector) {
        self.0.insert(message, selector);
    }

    #[inline]
    pub fn remove_selector(&mut self, message: MessageId) -> Option<Selector> {
        self.0.remove(&message)
    }

    #[inline]
    pub fn selector(&self, message: MessageId) -> Option<&Selector> {
        self.0.get(&message)
    }

    #[inline]
    pub fn is_selector(&self, message: MessageId) -> bool {
        self.0.contains_key(&message)
    }
}

pub async fn add_reaction(ctx: Context, reaction: Reaction) -> serenity::Result<()> {
    let (guild, user) = match (reaction.guild_id, reaction.user_id) {
        (Some(guild), Some(user)) => (guild, user),
        _ => return Ok(()),
    };

    let data = ctx.data.read().await;
    let messages = data.get::<StateKey>().unwrap();

    if let Some(selector) = messages.selector(reaction.message_id) {
        let emoji = reaction.emoji.clone().into();
        match selector.get_role(&emoji) {
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
    let messages = data.get::<StateKey>().unwrap();

    if let Some(selector) = messages.selector(reaction.message_id) {
        let emoji = reaction.emoji.clone().into();
        if let Some(role) = selector.get_role(&emoji) {
            let mut member: Member = guild.member(ctx, user).await?;
            member.remove_role(&ctx.http, role).await?;
        }
    }

    Ok(())
}

async fn is_message_selector(ctx: &Context, message: MessageId) -> bool {
    let data = ctx.data.read().await;
    let messages = data.get::<StateKey>().unwrap();

    messages.is_selector(message)
}

pub async fn delete_message(ctx: Context, message: MessageId) {
    if !is_message_selector(&ctx, message).await {
        return;
    }

    let mut data = ctx.data.write().await;
    let messages = data.get_mut::<StateKey>().unwrap();

    messages.write(|messages| {
        messages.remove_selector(message);
    }).await;
}

pub async fn update_message(mut ctx: Context, channel: ChannelId, message: MessageId, content: Option<String>) {
    if let Some(content) = content {
        if !is_message_selector(&ctx, message).await {
            return;
        }

        {
            let mut data = ctx.data.write().await;
            let messages = data.get_mut::<StateKey>().unwrap();

            messages.write(|messages| {
                messages.insert_selector(message, Selector::parse(&content));
            }).await;
        }

        apply_selector_reactions(&mut ctx, channel, message).await;
    }
}

async fn apply_selector_reactions(ctx: &Context, channel: ChannelId, message: MessageId) {
    let data = ctx.data.read().await;
    let messages = data.get::<StateKey>().unwrap();

    if let Some(selector) = messages.selector(message) {
        if let Ok(target_message) = channel.message(&ctx.http, message).await {
            let current_user = ctx.cache.current_user_id().await;

            let own_reactions: Vec<selector::Emoji> = target_message.reactions.iter()
                .filter(|reaction| reaction.me)
                .map(|reaction| selector::Emoji::from(reaction.reaction_type.clone()))
                .collect();

            for reaction in &own_reactions {
                if !selector.contains(reaction) {
                    let reaction_type = reaction.clone().into();
                    let _ = ctx.http.delete_reaction(channel.0, message.0, Some(current_user.0), &reaction_type).await;
                }
            }

            for (emoji, _) in selector.iter() {
                if !own_reactions.contains(emoji) {
                    let _ = target_message.react(ctx, emoji.clone()).await;
                }
            }
        }
    }
}

pub async fn add_selector(ctx: &Context, command: &Message, message_id: MessageId) -> CommandResult<()> {
    command.delete(ctx).await?;

    if let Ok(target_message) = command.channel_id.message(&ctx.http, message_id).await {
        {
            let mut data = ctx.data.write().await;
            let messages = data.get_mut::<StateKey>().unwrap();
            messages.write(|messages| {
                let selector = Selector::parse(&target_message.content);
                messages.insert_selector(message_id, selector);
            }).await;
        }

        apply_selector_reactions(ctx, command.channel_id, message_id).await;

        Ok(())
    } else {
        Err(CommandError::InvalidMessageReference)
    }
}

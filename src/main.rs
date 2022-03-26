// TODO: use slash commands
use std::str::FromStr;

use async_trait::async_trait;
use log::{error, info};
use serde::{Deserialize, Serialize};
use serenity::client::bridge::gateway::GatewayIntents;
use serenity::model::prelude::*;
use serenity::prelude::*;

pub use persistent::*;

mod persistent;
mod reaction_roles;
mod persistent_roles;

#[derive(Serialize, Deserialize, Default, Clone, Eq, PartialEq)]
pub struct Config {
    pub discord_token: String,
}

#[tokio::main]
async fn main() {
    env_logger::init();

    let config: Persistent<Config> = Persistent::open("config.json").await;

    let mut client = Client::builder(&config.discord_token)
        .event_handler(Handler)
        .intents(
            GatewayIntents::GUILD_MESSAGE_REACTIONS
                | GatewayIntents::GUILD_MESSAGES
                | GatewayIntents::GUILDS
                | GatewayIntents::GUILD_MEMBERS
        )
        .await
        .expect("failed to create client");

    {
        let mut data = client.data.write().await;
        data.insert::<reaction_roles::StateKey>(Persistent::open("reaction_roles.json").await);
        data.insert::<persistent_roles::StateKey>(Persistent::open("persistent_roles.json").await);
    }

    client.start().await.expect("failed to run client");
}

struct Handler;

#[async_trait]
impl EventHandler for Handler {
    async fn guild_member_addition(&self, ctx: Context, _guild_id: GuildId, mut member: Member) {
        persistent_roles::guild_member_addition(&ctx, &mut member).await;
    }

    async fn guild_member_update(&self, ctx: Context, _old: Option<Member>, member: Member) {
        persistent_roles::guild_member_update(&ctx, &member).await;
    }

    async fn message(&self, ctx: Context, message: Message) {
        if let Ok(true) = message.mentions_me(&ctx).await {
            let tokens: Vec<&str> = message.content.split_ascii_whitespace().collect();
            handle_command(&tokens[1..], &ctx, &message).await;
        }
    }

    async fn message_delete(&self, ctx: Context, _channel_id: ChannelId, deleted_message_id: MessageId, _guild_id: Option<GuildId>) {
        reaction_roles::delete_message(ctx, deleted_message_id).await;
    }

    async fn message_update(&self, ctx: Context, _old_if_available: Option<Message>, _new: Option<Message>, event: MessageUpdateEvent) {
        reaction_roles::update_message(ctx, event.channel_id, event.id, event.content).await;
    }

    async fn reaction_add(&self, ctx: Context, reaction: Reaction) {
        if let Err(err) = reaction_roles::add_reaction(ctx, reaction).await {
            error!("failed to add reaction role: {:?}", err);
        }
    }

    async fn reaction_remove(&self, ctx: Context, reaction: Reaction) {
        if let Err(err) = reaction_roles::remove_reaction(&ctx, reaction).await {
            error!("failed to remove reaction role: {:?}", err);
        }
    }

    async fn ready(&self, _ctx: Context, _ready: serenity::model::gateway::Ready) {
        info!("bot is ready!")
    }
}

async fn handle_command(tokens: &[&str], ctx: &Context, message: &Message) {
    let result = try_handle_command(tokens, ctx, message).await;

    let reaction = if result.is_ok() { "✅" } else { "❌" };
    let _ = message.react(&ctx, ReactionType::Unicode(reaction.to_owned())).await;

    if let Err(err) = result {
        let _ = message.reply(&ctx, err).await;
    }
}

async fn try_handle_command(tokens: &[&str], ctx: &Context, message: &Message) -> CommandResult<()> {
    let permissions = message_permissions(&ctx, &message).await;

    match tokens {
        ["add", "role", "selector", reference] => {
            require_permission(permissions, Permissions::MANAGE_ROLES)?;
            let reference = parse_argument(reference)?;
            reaction_roles::add_selector(&ctx, &message, MessageId(reference)).await
        }
        ["add", "role", "persist", refs @ ..] => {
            require_permission(permissions, Permissions::MANAGE_ROLES)?;
            for reference in refs {
                let reference = parse_argument(reference)?;
                persistent_roles::add_role(&ctx, &message, RoleId(reference)).await?;
            }
            Ok(())
        }
        ["remove", "role", "persist", refs @ ..] => {
            require_permission(permissions, Permissions::MANAGE_ROLES)?;
            for reference in refs {
                let reference = parse_argument(reference)?;
                persistent_roles::remove_role(&ctx, &message, RoleId(reference)).await?;
            }
            Ok(())
        }
        _ => Err(CommandError::InvalidCommand),
    }
}

fn parse_argument<T: FromStr>(argument: &str) -> CommandResult<T> {
    argument.parse::<T>().map_err(|_| CommandError::MalformedArgument(argument.to_owned()))
}

pub async fn message_permissions(ctx: &Context, message: &Message) -> Permissions {
    match message.guild_id {
        Some(guild_id) => member_permissions(ctx, guild_id, message.author.id).await,
        None => Permissions::empty(),
    }
}

pub async fn member_permissions(ctx: &Context, guild: GuildId, user: UserId) -> Permissions {
    if let Ok(member) = guild.member(ctx, user).await {
        if let Ok(permissions) = member.permissions(&ctx).await {
            return permissions;
        }
    }
    Permissions::empty()
}

#[inline]
fn require_permission(permissions: Permissions, require: Permissions) -> CommandResult<()> {
    if permissions.contains(require) {
        Ok(())
    } else {
        Err(CommandError::NoPermission(require))
    }
}

pub type CommandResult<T> = std::result::Result<T, CommandError>;

#[derive(thiserror::Error, Debug)]
pub enum CommandError {
    #[error("Discord error!")]
    Serenity(#[from] serenity::Error),
    #[error("Invalid command!")]
    InvalidCommand,
    #[error("You are not allowed to do this!")]
    NotAllowed,
    #[error("You are missing `{0}` permission!")]
    NoPermission(Permissions),
    #[error("Invalid message reference! Are you sure it's in this channel?")]
    InvalidMessageReference,
    #[error("Malformed argument: {0}")]
    MalformedArgument(String),
}

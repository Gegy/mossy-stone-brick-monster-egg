// TODO: use slash commands
use std::str::FromStr;

use async_trait::async_trait;
use log::{error, info};
use serde::{Deserialize, Serialize};
use serenity::client::bridge::gateway::GatewayIntents;
use serenity::model::prelude::*;
use serenity::prelude::*;

pub use name_filter::NameFilter;
pub use persistent::*;

mod persistent;
mod reaction_roles;
mod persistent_roles;
mod name_filter;

#[derive(Serialize, Deserialize, Default)]
pub struct Config {
    pub discord_token: String,
    pub ban_regex: Vec<String>,
}

#[tokio::main]
async fn main() {
    env_logger::init();

    let config: Persistent<Config> = Persistent::open("config.json").await;
    let name_filter = NameFilter::new(&config);

    let mut client = Client::builder(&config.discord_token)
        .event_handler(Handler { name_filter })
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

struct Handler {
    name_filter: NameFilter,
}

#[async_trait]
impl EventHandler for Handler {
    async fn guild_member_addition(&self, ctx: Context, guild_id: GuildId, mut member: Member) {
        if self.name_filter.is_illegal(&member.user.name) {
            let permissions = get_permissions(&ctx, guild_id, ctx.cache.current_user_id().await).await;
            if permissions.ban_members() {
                if let Err(err) = member.ban_with_reason(&ctx.http, 0, "Illegal username!").await {
                    error!("failed to ban user with illegal name! {:?}", err);
                }
            }
        }

        persistent_roles::guild_member_addition(&ctx, &mut member).await;
    }

    async fn guild_member_removal(&self, ctx: Context, _guild_id: GuildId, _user: User, member: Option<Member>) {
        if let Some(mut member) = member {
            persistent_roles::guild_member_removal(&ctx, &mut member).await;
        }
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
    let admin = check_message_admin(&ctx, &message).await;

    match tokens {
        ["add", "role", "selector", reference] if admin => {
            let reference = parse_argument(reference)?;
            reaction_roles::add_selector(&ctx, &message, MessageId(reference)).await
        }
        ["persist", "role", reference] if admin => {
            let reference = parse_argument(reference)?;
            persistent_roles::persist_role(&ctx, &message, RoleId(reference)).await
        }
        ["stop", "persist", "role", reference] if admin => {
            let reference = parse_argument(reference)?;
            persistent_roles::stop_persist_role(&ctx, &message, RoleId(reference)).await
        }
        _ => Err(CommandError::InvalidCommand),
    }
}

fn parse_argument<T: FromStr>(argument: &str) -> CommandResult<T> {
    argument.parse::<T>().map_err(|_| CommandError::MalformedArgument(argument.to_owned()))
}

pub async fn check_message_admin(ctx: &Context, message: &Message) -> bool {
    match message.guild_id {
        Some(guild_id) => get_permissions(ctx, guild_id, message.author.id).await.administrator(),
        None => false,
    }
}

async fn get_permissions(ctx: &Context, guild_id: GuildId, user_id: UserId) -> Permissions {
    if let Some(member) = ctx.cache.member(guild_id, user_id).await {
        if let Ok(permissions) = member.permissions(&ctx).await {
            return permissions;
        }
    }
    Permissions::empty()
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
    #[error("Invalid message reference! Are you sure it's in this channel?")]
    InvalidMessageReference,
    #[error("Malformed argument: {0}")]
    MalformedArgument(String),
}

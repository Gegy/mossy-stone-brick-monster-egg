use std::env;
use std::ops::Deref;
use std::path::PathBuf;

use async_trait::async_trait;
use log::{error, info};
use serde::{de::DeserializeOwned, Serialize};
use serenity::client::bridge::gateway::GatewayIntents;
use serenity::model::prelude::*;
use serenity::prelude::*;
use tokio::fs::File;
use tokio::prelude::*;

use reaction_roles::*;

mod reaction_roles;

pub struct Config<T: Serialize + DeserializeOwned + Default> {
    path: PathBuf,
    inner: T,
}

impl<T: Serialize + DeserializeOwned + Default> Config<T> {
    pub async fn open(path: impl Into<PathBuf>) -> Self {
        let path = path.into();

        let inner = if path.exists() {
            let mut file = File::open(&path).await.expect("failed to open file");

            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes).await.expect("failed to load file");

            serde_json::from_slice(&bytes).expect("failed to deserialize")
        } else {
            T::default()
        };

        Config { path, inner }
    }

    #[inline]
    pub async fn write<F, R>(&mut self, f: F) -> R
        where F: FnOnce(&mut T) -> R
    {
        let result = f(&mut self.inner);

        let mut file = File::create(&self.path).await.expect("failed to create file");

        let bytes = serde_json::to_vec(&self.inner).expect("failed to serialize");
        file.write_all(&bytes).await.expect("failed to write to file");

        result
    }

    #[inline]
    pub fn read(&self) -> &T {
        &self.inner
    }
}

impl<T: Serialize + DeserializeOwned + Default> Deref for Config<T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &T {
        &self.inner
    }
}

#[tokio::main]
async fn main() {
    env_logger::init();

    let token = env::var("DISCORD_TOKEN").expect("missing DISCORD_TOKEN");

    let mut client = Client::builder(token)
        .event_handler(Handler)
        .intents(GatewayIntents::GUILD_MESSAGE_REACTIONS | GatewayIntents::GUILD_MESSAGES | GatewayIntents::GUILDS)
        .await
        .expect("failed to create client");

    {
        let mut data = client.data.write().await;
        data.insert::<ReactionRoleKey>(Config::open("reaction_roles.json").await);
    }

    client.start().await.expect("failed to run client");
}

struct Handler;

#[async_trait]
impl EventHandler for Handler {
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
    let admin = check_message_admin(&ctx, &message).await;

    let result = match tokens {
        ["track", reference] if admin => {
            match reference.parse::<u64>() {
                Ok(reference) => reaction_roles::track_reactions(&ctx, &message, reference).await,
                Err(_) => Err(CommandError::MalformedArgument(reference.to_string())),
            }
        },
        _ => Err(CommandError::InvalidCommand),
    };

    let reaction = if result.is_ok() { "✅" } else { "❌" };
    let _ = message.react(&ctx, ReactionType::Unicode(reaction.to_owned())).await;

    if let Err(err) = result {
        let _ = message.reply(&ctx, err).await;
    }
}

pub async fn check_message_admin(ctx: &Context, message: &Message) -> bool {
    if let Ok(member) = message.member(&ctx).await {
        if let Ok(permissions) = member.permissions(&ctx).await {
            return permissions.administrator();
        }
    }
    false
}

pub type CommandResult = std::result::Result<(), CommandError>;

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
    MalformedArgument(String)
}

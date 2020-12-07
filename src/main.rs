use std::env;
use std::ops::Deref;
use std::path::PathBuf;

use async_trait::async_trait;
use log::{error, info};
use serde::{de::DeserializeOwned, Serialize};
use serenity::client::bridge::gateway::GatewayIntents;
use serenity::framework::standard::{Args, CheckResult, CommandOptions};
use serenity::framework::standard::macros::{check, group};
use serenity::framework::StandardFramework;
use serenity::model::prelude::*;
use serenity::prelude::*;
use tokio::fs::File;
use tokio::prelude::*;

use reaction_roles::*;

mod reaction_roles;

#[group]
#[only_in(guilds)]
#[checks(Admin)]
#[commands(track_reactions)]
struct ReactionRoles;

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

    let http = serenity::http::Http::new_with_token(&token);
    let info = http.get_current_application_info().await.expect("failed to get application info");

    let framework = StandardFramework::new()
        .configure(|c| {
            c.on_mention(Some(info.id))
                .case_insensitivity(true)
        })
        .group(&REACTIONROLES_GROUP);

    let mut client = Client::builder(token)
        .event_handler(Handler)
        .framework(framework)
        .intents(GatewayIntents::GUILD_MESSAGE_REACTIONS | GatewayIntents::GUILD_MESSAGES)
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
    async fn message_delete(&self, ctx: Context, _channel_id: ChannelId, deleted_message_id: MessageId) {
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

#[check]
#[name = "Admin"]
#[check_in_help(true)]
async fn admin_check(ctx: &Context, msg: &Message, _: &mut Args, _: &CommandOptions) -> CheckResult {
    if let Ok(member) = msg.member(ctx).await {
        if let Ok(permissions) = member.permissions(&ctx.cache).await {
            let administrator = permissions.administrator();
            return administrator.into();
        }
    }

    CheckResult::new_unknown()
}

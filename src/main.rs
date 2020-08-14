use std::env;
use std::fs::File;
use std::ops::Deref;
use std::path::PathBuf;

use log::{error, info};
use serde::{de::DeserializeOwned, Serialize};
use serenity::framework::standard::{Args, CheckResult, CommandOptions};
use serenity::framework::standard::macros::{check, group};
use serenity::framework::StandardFramework;
use serenity::model::prelude::*;
use serenity::prelude::*;

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
    pub fn open(path: impl Into<PathBuf>) -> Self {
        let path = path.into();

        let inner = if path.exists() {
            let file = File::open(&path).expect("failed to open file");
            serde_json::from_reader(file).expect("failed to deserialize")
        } else {
            T::default()
        };

        Config { path, inner }
    }

    #[inline]
    pub fn write<F, R>(&mut self, f: F) -> R
        where F: FnOnce(&mut T) -> R
    {
        let result = f(&mut self.inner);

        let file = File::create(&self.path).expect("failed to create file");
        serde_json::to_writer(file, &self.inner).expect("failed to serialize");

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

fn main() {
    env_logger::init();

    let token = env::var("DISCORD_TOKEN").expect("missing DISCORD_TOKEN");

    let mut client = Client::new(token, Handler).expect("failed to create client");

    let info = client.cache_and_http.http.get_current_application_info().expect("failed to get application info");

    {
        let mut data = client.data.write();
        data.insert::<ReactionRoleKey>(Config::open("reaction_roles.json"));
    }

    client.with_framework(StandardFramework::new()
        .configure(|c| {
            c.prefix("::")
                .on_mention(Some(info.id))
                .case_insensitivity(true)
        })
        .group(&REACTIONROLES_GROUP));

    client.start().expect("failed to run client");
}

struct Handler;

impl EventHandler for Handler {
    fn ready(&self, _ctx: Context, _ready: serenity::model::gateway::Ready) {
        info!("bot is ready!")
    }

    fn reaction_add(&self, ctx: Context, reaction: Reaction) {
        if let Err(err) = reaction_roles::add_reaction(ctx, reaction) {
            error!("failed to add reaction role: {:?}", err);
        }
    }

    fn reaction_remove(&self, ctx: Context, reaction: Reaction) {
        if let Err(err) = reaction_roles::remove_reaction(ctx, reaction) {
            error!("failed to remove reaction role: {:?}", err);
        }
    }

    fn message_delete(&self, ctx: Context, _channel_id: ChannelId, deleted_message_id: MessageId) {
        reaction_roles::delete_message(ctx, deleted_message_id);
    }

    fn message_update(&self, ctx: Context, _old_if_available: Option<Message>, _new: Option<Message>, event: MessageUpdateEvent) {
        reaction_roles::update_message(ctx, event.channel_id, event.id, event.content);
    }
}

#[check]
#[name = "Admin"]
#[check_in_help(true)]
fn admin_check(ctx: &mut Context, msg: &Message, _: &mut Args, _: &CommandOptions) -> CheckResult {
    if let Some(member) = msg.member(&ctx.cache) {
        if let Ok(permissions) = member.permissions(&ctx.cache) {
            let administrator = permissions.administrator();
            return administrator.into();
        }
    }

    CheckResult::new_unknown()
}

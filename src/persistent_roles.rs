use std::collections::{HashMap, HashSet};
use std::future;
use std::time::Duration;

use log::error;
use serde::{Deserialize, Serialize};
use serenity::futures::TryStreamExt;
use serenity::model::prelude::*;
use serenity::prelude::*;

use crate::{CommandError, CommandResult, Persistent};

pub struct StateKey;

impl TypeMapKey for StateKey {
    type Value = Persistent<State>;
}

#[derive(Serialize, Deserialize, Default, Clone, Eq, PartialEq)]
pub struct State {
    guilds: HashMap<GuildId, GuildState>,
}

#[derive(Serialize, Deserialize, Default, Clone, Eq, PartialEq)]
struct GuildState {
    roles: HashSet<RoleId>,
    users: HashMap<UserId, Vec<RoleId>>,
}

impl GuildState {
    pub fn set_user_roles(&mut self, user: UserId, roles: Vec<RoleId>) {
        if !roles.is_empty() {
            self.users.insert(user, roles);
        } else {
            self.users.remove(&user);
        }
    }

    pub fn add_role(&mut self, role: RoleId, users_with_role: Vec<UserId>) {
        if self.roles.insert(role) {
            for user in users_with_role {
                let roles = self.users.entry(user).or_insert_with(|| Vec::new());
                roles.push(role);
            }
        }
    }

    pub fn remove_role(&mut self, role: RoleId) {
        if self.roles.remove(&role) {
            let mut empty_users = Vec::new();

            for (user, roles) in &mut self.users {
                if let Some(index) = roles.iter().position(|r| *r == role) {
                    roles.swap_remove(index);
                }

                if roles.is_empty() {
                    empty_users.push(*user);
                }
            }

            for user in empty_users {
                self.users.remove(&user);
            }
        }
    }
}

pub async fn add_role(ctx: &Context, command: &Message, role: RoleId) -> CommandResult<()> {
    if let Some(guild) = command.guild_id {
        let mut data = ctx.data.write().await;

        let users_with_role = users_with_role(ctx, guild, role).await?;

        let state = data.get_mut::<StateKey>().unwrap();
        state.write(|state| {
            let guild = state.guilds.entry(guild).or_insert_with(|| GuildState::default());
            guild.add_role(role, users_with_role);
        }).await;

        Ok(())
    } else {
        Err(CommandError::NotAllowed)
    }
}

async fn users_with_role(ctx: &Context, guild: GuildId, role: RoleId) -> serenity::Result<Vec<UserId>> {
    guild.members_iter(ctx)
        .try_filter(|member| future::ready(member.roles.contains(&role)))
        .map_ok(|member| member.user.id)
        .try_collect()
        .await
}

pub async fn remove_role(ctx: &Context, command: &Message, role: RoleId) -> CommandResult<()> {
    if let Some(guild) = command.guild_id {
        let mut data = ctx.data.write().await;

        let state = data.get_mut::<StateKey>().unwrap();
        state.write(|state| {
            if let Some(guild) = state.guilds.get_mut(&guild) {
                guild.remove_role(role);
            }
        }).await;

        Ok(())
    } else {
        Err(CommandError::NotAllowed)
    }
}

pub async fn guild_member_addition(ctx: &Context, member: &mut Member) {
    let data = ctx.data.read().await;
    let state = data.get::<StateKey>().unwrap();

    let roles = match state.guilds.get(&member.guild_id) {
        Some(guild) => guild.users.get(&member.user.id).cloned().unwrap_or_default(),
        None => Vec::default()
    };

    if !roles.is_empty() {
        let permissions = crate::member_permissions(ctx, member.guild_id, ctx.cache.current_user_id().await).await;
        if !permissions.manage_roles() {
            return;
        }

        // magic delay to make sure adding the roles actually does so
        tokio::time::sleep(Duration::from_secs(1)).await;

        if let Err(err) = member.add_roles(&ctx, &roles).await {
            error!("failed to add persisted roles ({:?}) to {}: {:?}", roles, member, err);
        }
    }
}

pub async fn guild_member_update(ctx: &Context, member: &Member) {
    if !has_guild(ctx, member.guild_id).await {
        return;
    }

    let mut data = ctx.data.write().await;
    let state = data.get_mut::<StateKey>().unwrap();

    state.write(|state| {
        if let Some(guild) = state.guilds.get_mut(&member.guild_id) {
            let roles = member.roles.iter()
                .filter(|role| guild.roles.contains(role))
                .cloned()
                .collect();

            guild.set_user_roles(member.user.id, roles);
        }
    }).await;
}

async fn has_guild(ctx: &Context, guild: GuildId) -> bool {
    let data = ctx.data.read().await;
    let state = data.get::<StateKey>().unwrap();
    state.guilds.contains_key(&guild)
}

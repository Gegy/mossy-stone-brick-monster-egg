use std::collections::{HashMap, HashSet};
use std::future;

use log::error;
use serde::{Deserialize, Serialize};
use serenity::futures::TryStreamExt;
use serenity::model::prelude::*;
use serenity::prelude::*;

use crate::{CommandError, CommandResult, Persistent};
use std::time::Duration;

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

pub async fn add_role(ctx: &Context, command: &Message, role: RoleId) -> CommandResult<()> {
    if let Some(guild) = command.guild_id {
        let mut data = ctx.data.write().await;

        let users_with_role: Vec<UserId> = guild.members_iter(ctx)
            .try_filter(|member| future::ready(member.roles.contains(&role)))
            .map_ok(|member| member.user.id)
            .try_collect()
            .await?;

        let state = data.get_mut::<StateKey>().unwrap();
        state.write(|state| {
            add_role_to(role, guild, state, users_with_role);
        }).await;

        Ok(())
    } else {
        Err(CommandError::NotAllowed)
    }
}

pub async fn remove_role(ctx: &Context, command: &Message, role: RoleId) -> CommandResult<()> {
    if let Some(guild) = command.guild_id {
        let mut data = ctx.data.write().await;

        let state = data.get_mut::<StateKey>().unwrap();
        state.write(|state| {
            if let Some(guild) = state.guilds.get_mut(&guild) {
                remove_role_from(&role, guild)
            }
        }).await;

        Ok(())
    } else {
        Err(CommandError::NotAllowed)
    }
}

fn add_role_to(role: RoleId, guild: GuildId, state: &mut State, users_with_role: Vec<UserId>) {
    let guild = state.guilds.entry(guild).or_insert_with(|| GuildState::default());
    guild.roles.insert(role);

    for user in users_with_role {
        let roles = guild.users.entry(user).or_insert_with(|| Vec::new());
        roles.push(role);
    }
}

fn remove_role_from(role: &RoleId, guild: &mut GuildState) {
    guild.roles.remove(&role);

    for (_, roles) in &mut guild.users {
        if let Some(index) = roles.iter().position(|r| r == role) {
            roles.swap_remove(index);
        }
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
            guild.users.insert(member.user.id, roles);
        }
    }).await;
}

async fn has_guild(ctx: &Context, guild: GuildId) -> bool {
    let data = ctx.data.read().await;
    let state = data.get::<StateKey>().unwrap();
    state.guilds.contains_key(&guild)
}

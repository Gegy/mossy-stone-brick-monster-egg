use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use serenity::model::prelude::*;
use serenity::prelude::*;

use crate::{CommandError, CommandResult, Persistent};

pub struct StateKey;

impl TypeMapKey for StateKey {
    type Value = Persistent<State>;
}

#[derive(Serialize, Deserialize, Default)]
pub struct State {
    guilds: HashMap<GuildId, GuildState>,
}

#[derive(Serialize, Deserialize, Default)]
struct GuildState {
    roles: HashSet<RoleId>,
    users: HashMap<UserId, Vec<RoleId>>,
}

impl GuildState {}

pub async fn persist_role(ctx: &Context, command: &Message, role: RoleId) -> CommandResult<()> {
    if let Some(guild) = command.guild_id {
        command.delete(ctx).await?;

        let mut data = ctx.data.write().await;

        let state = data.get_mut::<StateKey>().unwrap();
        state.write(|state| {
            add_role(role, guild, state);
        }).await;

        Ok(())
    } else {
        Err(CommandError::NotAllowed)
    }
}

pub async fn stop_persist_role(ctx: &Context, command: &Message, role: RoleId) -> CommandResult<()> {
    if let Some(guild) = command.guild_id {
        command.delete(ctx).await?;

        let mut data = ctx.data.write().await;

        let state = data.get_mut::<StateKey>().unwrap();
        state.write(|state| {
            if let Some(guild) = state.guilds.get_mut(&guild) {
                remove_role(&role, guild)
            }
        }).await;

        Ok(())
    } else {
        Err(CommandError::NotAllowed)
    }
}

fn add_role(role: RoleId, guild: GuildId, state: &mut State) {
    let guild = state.guilds.entry(guild).or_insert_with(|| GuildState::default());
    guild.roles.insert(role);
}

fn remove_role(role: &RoleId, guild: &mut GuildState) {
    guild.roles.remove(&role);

    for (_, roles) in &mut guild.users {
        if let Some(index) = roles.iter().position(|r| r == role) {
            roles.swap_remove(index);
        }
    }
}

pub async fn guild_member_addition(ctx: &Context, member: &mut Member) {
    if !has_guild(ctx, member.guild_id).await {
        return;
    }

    let manage_roles = member.permissions(ctx).await
        .map(|permissions| permissions.manage_roles())
        .unwrap_or(false);

    if !manage_roles {
        return;
    }

    let mut data = ctx.data.write().await;
    let state = data.get_mut::<StateKey>().unwrap();

    let roles = state.write(|state| {
        if let Some(guild) = state.guilds.get_mut(&member.guild_id) {
            guild.users.remove(&member.user.id)
        } else {
            None
        }
    }).await;

    if let Some(roles) = roles {
        let _ = member.add_roles(&ctx, &roles).await;
    }
}

pub async fn guild_member_removal(ctx: &Context, member: &mut Member) {
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

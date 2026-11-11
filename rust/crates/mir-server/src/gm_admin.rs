//! GM commands that need account records or connection ownership.

use mir_proto::{ChatKind, ObjectId, ServerMessage};
use mir_server::accounts::{now_secs, Accounts};
use mir_server::world::{ConnId, World};

use crate::{disconnect, Sessions, Stage};

pub(super) fn handle(
    world: &mut World,
    accounts: &mut Accounts,
    sessions: &mut Sessions,
    conn: ConnId,
    object: ObjectId,
    text: &str,
) -> bool {
    let Some(command) = text.trim().strip_prefix('@') else {
        return false;
    };
    let words: Vec<&str> = command.split_whitespace().collect();
    let name = words.first().copied().unwrap_or("").to_ascii_lowercase();
    if !matches!(
        name.as_str(),
        "ban"
            | "chatban"
            | "globalban"
            | "kick"
            | "givegamegold"
            | "takegamegold"
            | "clearipblocks"
            | "gcollect"
            | "removecaption"
    ) {
        return false;
    }
    if !world
        .objects
        .get(&object)
        .and_then(|o| o.player())
        .is_some_and(|p| p.gm)
    {
        reply(sessions, conn, "GM access required.");
        return true;
    }
    tracing::info!(conn, command = %name, "GM account command");
    let args = &words[1..];
    match run(world, accounts, sessions, conn, &name, args) {
        Ok((message, target)) => {
            reply(sessions, conn, &message);
            if let Some((target, reason)) = target {
                sessions.send(target, ServerMessage::Rejected { reason });
                disconnect(world, accounts, sessions, target);
            }
            if let Err(e) = accounts.save_if_dirty() {
                tracing::error!("saving GM account change failed: {e:#}");
            }
        }
        Err(error) => reply(sessions, conn, &error),
    }
    true
}

fn reply(sessions: &mut Sessions, conn: ConnId, message: &str) {
    sessions.send(
        conn,
        ServerMessage::Say {
            id: None,
            kind: ChatKind::System,
            text: message.to_string(),
        },
    );
}

fn online(sessions: &Sessions, account: u32) -> Option<(ConnId, ObjectId, u32)> {
    sessions
        .state
        .iter()
        .find_map(|(&conn, stage)| match stage {
            Stage::InGame {
                account: a,
                character,
                object,
            } if *a == account => Some((conn, *object, *character)),
            _ => None,
        })
}

fn target(accounts: &Accounts, name: &str) -> Result<(u32, u32, String), String> {
    accounts
        .find_character_full(name)
        .ok_or_else(|| format!("Unknown character: {name}"))
}

fn expiry(args: &[&str], usage: &str) -> Result<u64, String> {
    let minutes = match args {
        [_] => 365 * 24 * 60,
        [_, minutes] => minutes.parse::<u64>().map_err(|_| usage.to_string())?,
        _ => return Err(usage.into()),
    };
    if minutes == 0 || minutes > 10 * 365 * 24 * 60 {
        return Err("Ban duration must be 1 minute to 10 years.".into());
    }
    Ok(now_secs().saturating_add(minutes.saturating_mul(60)))
}

type DropConnection = Option<(ConnId, String)>;

fn run(
    world: &mut World,
    accounts: &mut Accounts,
    sessions: &Sessions,
    conn: ConnId,
    name: &str,
    args: &[&str],
) -> Result<(String, DropConnection), String> {
    match name {
        "ban" | "chatban" | "globalban" => {
            let until = expiry(args, &format!("Usage: @{name} <player> [minutes]"))?;
            let (account, _, proper) = target(accounts, args[0])?;
            let acc = accounts.account_mut(account).unwrap();
            match name {
                "ban" => acc.banned_until = until,
                "chatban" => acc.chat_ban_until = until,
                _ => acc.global_ban_until = until,
            }
            let (chat, global) = (acc.chat_ban_until, acc.global_ban_until);
            accounts.mark_dirty();
            if let Some((_, object, _)) = online(sessions, account) {
                world.set_chat_bans(object, chat, global);
            }
            let drop = if name == "ban" {
                online(sessions, account).map(|(conn, _, _)| (conn, "Account banned by GM.".into()))
            } else {
                None
            };
            Ok((format!("{name} applied to {proper}."), drop))
        }
        "kick" => {
            let [name] = args else {
                return Err("Usage: @kick <player>".into());
            };
            let (account, _, proper) = target(accounts, name)?;
            let (target_conn, _, _) = online(sessions, account).ok_or("Player is not online")?;
            if target_conn == conn {
                return Err("You cannot kick yourself.".into());
            }
            Ok((
                format!("Kicked {proper}."),
                Some((target_conn, "Kicked by GM.".into())),
            ))
        }
        "givegamegold" | "takegamegold" => {
            let [player_name, amount] = args else {
                return Err(format!("Usage: @{name} <player> <amount>"));
            };
            let amount = amount.parse::<i64>().map_err(|_| "Invalid amount")?;
            if amount <= 0 {
                return Err("Amount must be positive.".into());
            }
            let currency = world
                .data
                .currencies
                .iter()
                .find(|c| c.currency_type == 1)
                .map(|c| c.index)
                .ok_or("Game Gold currency is not configured")?;
            let (account, character, proper) = target(accounts, player_name)?;
            let rec = accounts.character_mut(account, character).unwrap();
            let entry = rec
                .currencies
                .iter_mut()
                .find(|(index, _)| *index == currency);
            let new_amount = if let Some((_, value)) = entry {
                *value = if name == "givegamegold" {
                    value.saturating_add(amount)
                } else {
                    value.saturating_sub(amount).max(0)
                };
                *value
            } else {
                let value = if name == "givegamegold" { amount } else { 0 };
                rec.currencies.push((currency, value));
                value
            };
            accounts.mark_dirty();
            if let Some((_, object, playing)) = online(sessions, account) {
                if playing == character {
                    world
                        .objects
                        .get_mut(&object)
                        .and_then(|o| o.player_mut())
                        .unwrap()
                        .currencies
                        .insert(currency, new_amount);
                    world.send_currencies(object);
                }
            }
            Ok((
                format!("{name} {amount} for {proper}; balance {new_amount}."),
                None,
            ))
        }
        "removecaption" => {
            let [name] = args else {
                return Err("Usage: @removecaption <player>".into());
            };
            let (account, character, proper) = target(accounts, name)?;
            accounts.character_mut(account, character).unwrap().caption = None;
            accounts.mark_dirty();
            Ok((format!("Removed {proper}'s caption."), None))
        }
        "clearipblocks" if args.is_empty() => {
            Ok(("No persistent IP blocks are configured.".into(), None))
        }
        "gcollect" if args.is_empty() => {
            Ok(("Rust releases unused memory automatically.".into(), None))
        }
        _ => Err(format!("Usage: @{name}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mir_proto::{Class, Gender};
    use mir_server::data::GameData;

    #[test]
    fn account_commands_are_gated_persisted_and_apply_to_online_players() {
        let Some(assets) = std::env::var_os("ZIRCON_ASSETS") else {
            return;
        };
        let assets = std::path::PathBuf::from(assets);
        let dir = std::env::temp_dir().join(format!("zircon-gm-admin-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut accounts = Accounts::load(dir.join("accounts.json")).unwrap();
        let gm_account = accounts.create("master@example.test", "secret123").unwrap();
        let target_account = accounts.create("target@example.test", "secret123").unwrap();
        let gm_rec = accounts
            .new_character(gm_account, "Master", Class::Warrior, Gender::Male, 1)
            .unwrap();
        let target_rec = accounts
            .new_character(target_account, "Target", Class::Warrior, Gender::Male, 1)
            .unwrap();
        accounts.character_mut(gm_account, gm_rec.id).unwrap().gm = true;
        accounts.mark_dirty();
        let data = GameData::load(assets.join("../Database/System.db")).unwrap();
        let mut world = World::new(data, assets.join("Map"), None);
        let gm = world
            .add_player(
                1,
                gm_account,
                accounts.character(gm_account, gm_rec.id).unwrap(),
            )
            .unwrap();
        let target = world.add_player(2, target_account, &target_rec).unwrap();
        let mut sessions = Sessions {
            conns: Default::default(),
            state: Default::default(),
            since: Default::default(),
            counts: Default::default(),
            dropped: Vec::new(),
            login_at: Default::default(),
            save_soon: false,
        };
        sessions.state.insert(
            1,
            Stage::InGame {
                account: gm_account,
                character: gm_rec.id,
                object: gm,
            },
        );
        sessions.state.insert(
            2,
            Stage::InGame {
                account: target_account,
                character: target_rec.id,
                object: target,
            },
        );

        assert!(handle(
            &mut world,
            &mut accounts,
            &mut sessions,
            2,
            target,
            "@ban Master"
        ));
        assert_eq!(accounts.account(gm_account).unwrap().banned_until, 0);
        assert!(handle(
            &mut world,
            &mut accounts,
            &mut sessions,
            1,
            gm,
            "@chatban Target 5"
        ));
        assert!(world.objects[&target].player().unwrap().chat_ban_until > now_secs());
        assert!(handle(
            &mut world,
            &mut accounts,
            &mut sessions,
            1,
            gm,
            "@givegamegold Target 50"
        ));
        let currency = world
            .data
            .currencies
            .iter()
            .find(|c| c.currency_type == 1)
            .unwrap()
            .index;
        assert_eq!(
            world.objects[&target].player().unwrap().currencies[&currency],
            50
        );
        assert!(handle(
            &mut world,
            &mut accounts,
            &mut sessions,
            1,
            gm,
            "@ban Target 5"
        ));
        assert!(!sessions.state.contains_key(&2));
        assert_eq!(
            accounts.login("target@example.test", "secret123"),
            Err("Account banned".into())
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}

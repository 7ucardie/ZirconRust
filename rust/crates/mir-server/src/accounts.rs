//! Account and character persistence: a JSON file with Argon2 password hashes.
//!
//! Writes are atomic (temp file + rename). The store is small and fully in
//! memory; it is saved on every account-level change and periodically.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use argon2::password_hash::{
    rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString,
};
use argon2::Argon2;
use mir_proto::{
    rules, BeltLink, CharacterSummary, Class, Direction, Gender, Point, MAX_PLAYER_LEVEL,
};
use serde::{Deserialize, Serialize};

use crate::items::StoredItem;
use crate::magic::StoredMagic;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CharacterRecord {
    pub id: u32,
    pub name: String,
    pub class: Class,
    pub gender: Gender,
    pub hair: u8,
    pub level: i32,
    pub experience: u64,
    /// Fractional experience carried between awards (C# stores decimal XP).
    #[serde(default)]
    pub experience_fraction: f64,
    pub hp: i32,
    pub mp: i32,
    /// Map file stem; empty until the character has entered the world.
    pub map: String,
    pub location: Point,
    pub direction: Direction,
    pub created: u64,
    pub last_login: u64,
    pub deleted: bool,
    #[serde(default)]
    pub gm: bool,
    #[serde(default)]
    pub caption: Option<String>,
    #[serde(default)]
    pub items: Vec<StoredItem>,
    /// Character bag capacity; old records use the base 48 slots.
    #[serde(default)]
    pub inventory_size: u8,
    #[serde(default)]
    pub gold: u64,
    /// Event slots already credited. Saved with Gold to make pending grants idempotent.
    #[serde(default)]
    pub event_rewards: Vec<u64>,
    /// Highest item id handed out for this character.
    #[serde(default)]
    pub next_item_id: u32,
    #[serde(default)]
    pub magics: Vec<StoredMagic>,
    #[serde(default)]
    pub belt: Vec<BeltLink>,
    /// Zircon `UserCurrency`: (CurrencyInfo index, amount); gold lives in `gold`.
    #[serde(default)]
    pub currencies: Vec<(i32, i64)>,
    #[serde(default)]
    pub rebirth: i32,
    #[serde(default)]
    pub quests: Vec<StoredQuest>,
    /// Zircon `AllowGroup` (per character here, per account in Zircon).
    #[serde(default)]
    pub allow_group: bool,
    #[serde(default)]
    pub attack_mode: u8,
    /// Zircon `Character.PetMode`; old saves default to Move + Attack.
    #[serde(default)]
    pub pet_mode: u8,
    #[serde(default)]
    pub pk_points: i32,
    /// Zircon `Character.Fame`: the held `FameInfo` index (0 = none).
    #[serde(default)]
    pub fame: i32,
    #[serde(default)]
    pub hermit_stats: Vec<(i32, i32)>,
    /// Zircon `Account.Horse` (per character here).
    #[serde(default)]
    pub horse: u8,
    /// Partner character id and name (0 / "" when unmarried).
    #[serde(default)]
    pub partner: u32,
    #[serde(default)]
    pub partner_name: String,
    /// Item id of the wedding ring (0 = none).
    #[serde(default)]
    pub wedding_ring: u32,
    /// Weapons in the furnace (Zircon `RefineInfo`).
    #[serde(default)]
    pub refines: Vec<crate::world::StoredRefine>,
    /// Zircon `UserCompanion`s (per character here, per account in Zircon).
    #[serde(default)]
    pub companions: Vec<crate::world::StoredCompanion>,
    #[serde(default)]
    pub active_companion: Option<u32>,
    /// `CompanionInfo` indices unlocked with their items.
    #[serde(default)]
    pub companion_unlocks: Vec<i32>,
    /// Zircon `Character.HairColour`, packed ARGB (0 = the sprite's own).
    #[serde(default)]
    pub hair_colour: u32,
    /// Cosmetic armour ItemInfo index; never contributes item stats.
    #[serde(default)]
    pub wardrobe_item: Option<i32>,
    #[serde(default)]
    pub wardrobe_helmet: Option<i32>,
    #[serde(default)]
    pub wardrobe_weapon: Option<i32>,
    #[serde(default)]
    pub wardrobe_shoes: Option<i32>,
    /// Zircon `UserDiscipline.Level` (0 = no discipline taken) and the
    /// experience banked towards the next rung.
    #[serde(default)]
    pub discipline: i32,
    #[serde(default)]
    pub discipline_experience: i64,
}

/// Zircon `UserQuest` as persisted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredQuest {
    pub quest: i32,
    pub completed: bool,
    pub track: bool,
    pub selected_reward: i32,
    /// Unix seconds.
    pub taken: u64,
    pub completed_at: u64,
    /// `(QuestTask index, amount)`.
    pub tasks: Vec<(i32, i32)>,
}

impl CharacterRecord {
    pub fn summary(&self) -> CharacterSummary {
        CharacterSummary {
            id: self.id,
            name: self.name.clone(),
            class: self.class,
            gender: self.gender,
            hair: self.hair,
            level: self.level.clamp(1, MAX_PLAYER_LEVEL),
            last_login: self.last_login,
            location: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountRecord {
    pub id: u32,
    pub email: String,
    pub password_hash: String,
    pub created: u64,
    pub last_login: u64,
    /// Unix seconds. Zero means no active ban.
    #[serde(default)]
    pub banned_until: u64,
    #[serde(default)]
    pub chat_ban_until: u64,
    #[serde(default)]
    pub global_ban_until: u64,
    pub characters: Vec<CharacterRecord>,
    /// Account storage (Zircon `Account.Items` with storage slots).
    #[serde(default)]
    pub storage: Vec<StoredItem>,
    #[serde(default)]
    pub storage_size: u32,
    /// Drop progress per item and the fortune snapshots read from it
    /// (Zircon `Account.UserDrops` / `Account.Fortunes`).
    #[serde(default)]
    pub drops: Vec<crate::world::fortune::DropProgress>,
    #[serde(default)]
    pub fortunes: Vec<crate::world::fortune::FortuneRecord>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Store {
    next_account_id: u32,
    next_character_id: u32,
    accounts: Vec<AccountRecord>,
}

pub struct Accounts {
    path: PathBuf,
    store: Store,
    dirty: bool,
    /// lower-cased email -> index into `store.accounts`
    by_email: HashMap<String, usize>,
}

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl Accounts {
    pub fn load(path: impl AsRef<Path>) -> Result<Accounts> {
        let path = path.as_ref().to_path_buf();
        let store = if path.exists() {
            let text = std::fs::read_to_string(&path)
                .with_context(|| format!("reading {}", path.display()))?;
            serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))?
        } else {
            Store {
                next_account_id: 1,
                next_character_id: 1,
                accounts: Vec::new(),
            }
        };
        let by_email = store
            .accounts
            .iter()
            .enumerate()
            .map(|(i, a)| (a.email.to_lowercase(), i))
            .collect();
        tracing::info!(path = %path.display(), accounts = store.accounts.len(), "accounts loaded");
        Ok(Accounts {
            path,
            store,
            dirty: false,
            by_email,
        })
    }

    pub fn save_if_dirty(&mut self) -> Result<()> {
        if !self.dirty {
            return Ok(());
        }
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        let text = serde_json::to_string_pretty(&self.store)?;
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        std::fs::rename(&tmp, &self.path)?;
        #[cfg(unix)]
        if let Some(dir) = self.path.parent() {
            std::fs::File::open(dir)?.sync_all()?;
        }
        self.dirty = false;
        tracing::debug!(path = %self.path.display(), "accounts saved");
        Ok(())
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    pub fn create(&mut self, email: &str, password: &str) -> std::result::Result<u32, String> {
        let email = email.trim();
        if !rules::valid_email(email) {
            return Err("Invalid email address".into());
        }
        if !rules::valid_password(password) {
            return Err(format!(
                "Password must be {}-{} characters",
                rules::PASSWORD_MIN,
                rules::PASSWORD_MAX
            ));
        }
        if self.by_email.contains_key(&email.to_lowercase()) {
            return Err("Account already exists".into());
        }
        let salt = SaltString::generate(&mut OsRng);
        let hash = Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map_err(|e| format!("hashing failed: {e}"))?
            .to_string();
        let id = self.store.next_account_id;
        self.store.next_account_id += 1;
        self.store.accounts.push(AccountRecord {
            id,
            email: email.to_string(),
            password_hash: hash,
            created: now_secs(),
            last_login: 0,
            banned_until: 0,
            chat_ban_until: 0,
            global_ban_until: 0,
            characters: Vec::new(),
            storage: Vec::new(),
            storage_size: 0,
            drops: Vec::new(),
            fortunes: Vec::new(),
        });
        self.by_email
            .insert(email.to_lowercase(), self.store.accounts.len() - 1);
        self.dirty = true;
        Ok(id)
    }

    /// Verify credentials and return the account id.
    pub fn login(&mut self, email: &str, password: &str) -> std::result::Result<u32, String> {
        let idx = *self
            .by_email
            .get(&email.trim().to_lowercase())
            .ok_or_else(|| "Account not found".to_string())?;
        let account = &mut self.store.accounts[idx];
        let parsed = PasswordHash::new(&account.password_hash).map_err(|e| e.to_string())?;
        if Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_err()
        {
            return Err("Wrong password".into());
        }
        if account.banned_until > now_secs() {
            return Err("Account banned".into());
        }
        account.last_login = now_secs();
        self.dirty = true;
        Ok(account.id)
    }

    pub fn account(&self, id: u32) -> Option<&AccountRecord> {
        self.store.accounts.iter().find(|a| a.id == id)
    }

    pub fn account_mut(&mut self, id: u32) -> Option<&mut AccountRecord> {
        self.store.accounts.iter_mut().find(|a| a.id == id)
    }

    pub fn summaries(&self, account: u32) -> Vec<CharacterSummary> {
        self.account(account)
            .map(|a| {
                a.characters
                    .iter()
                    .filter(|c| !c.deleted)
                    .map(|c| c.summary())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn new_character(
        &mut self,
        account: u32,
        name: &str,
        class: Class,
        gender: Gender,
        hair: u8,
    ) -> std::result::Result<CharacterRecord, String> {
        let name = name.trim();
        if !rules::valid_name(name) {
            return Err(format!(
                "Name must be {}-{} letters or digits and start with a letter",
                rules::NAME_MIN,
                rules::NAME_MAX
            ));
        }
        if hair == 0 || hair > rules::HAIR_TYPES {
            return Err("Invalid hair".into());
        }
        let taken = self.store.accounts.iter().any(|a| {
            a.characters
                .iter()
                .any(|c| !c.deleted && c.name.eq_ignore_ascii_case(name))
        });
        if taken {
            return Err("Name already taken".into());
        }
        let id = self.store.next_character_id;
        let acc = self
            .account_mut(account)
            .ok_or_else(|| "No such account".to_string())?;
        if acc.characters.iter().filter(|c| !c.deleted).count() >= rules::MAX_CHARACTERS {
            return Err(format!("Maximum {} characters", rules::MAX_CHARACTERS));
        }
        let rec = CharacterRecord {
            id,
            name: name.to_string(),
            class,
            gender,
            hair,
            level: 1,
            experience: 0,
            experience_fraction: 0.0,
            hp: 0,
            mp: 0,
            map: String::new(),
            location: Point::default(),
            direction: Direction::Down,
            created: now_secs(),
            last_login: 0,
            deleted: false,
            gm: false,
            caption: None,
            items: Vec::new(),
            inventory_size: mir_proto::INVENTORY_SIZE as u8,
            gold: 0,
            event_rewards: Vec::new(),
            next_item_id: 0,
            magics: Vec::new(),
            belt: Vec::new(),
            currencies: Vec::new(),
            rebirth: 0,
            quests: Vec::new(),
            allow_group: false,
            attack_mode: 0,
            pet_mode: 0,
            pk_points: 0,
            fame: 0,
            hermit_stats: Vec::new(),
            horse: 0,
            partner: 0,
            partner_name: String::new(),
            wedding_ring: 0,
            refines: Vec::new(),
            companions: Vec::new(),
            active_companion: None,
            companion_unlocks: Vec::new(),
            hair_colour: 0,
            wardrobe_item: None,
            wardrobe_helmet: None,
            wardrobe_weapon: None,
            wardrobe_shoes: None,
            discipline: 0,
            discipline_experience: 0,
        };
        acc.characters.push(rec.clone());
        self.store.next_character_id += 1;
        self.dirty = true;
        Ok(rec)
    }

    pub fn delete_character(&mut self, account: u32, id: u32) -> std::result::Result<(), String> {
        let acc = self
            .account_mut(account)
            .ok_or_else(|| "No such account".to_string())?;
        let c = acc
            .characters
            .iter_mut()
            .find(|c| c.id == id && !c.deleted)
            .ok_or_else(|| "No such character".to_string())?;
        c.deleted = true;
        self.dirty = true;
        Ok(())
    }

    /// Clear a character's marriage (divorced while offline).
    pub fn clear_partner(&mut self, character: u32) {
        for a in &mut self.store.accounts {
            for c in &mut a.characters {
                if c.id == character {
                    c.partner = 0;
                    c.partner_name.clear();
                    c.wedding_ring = 0;
                    self.dirty = true;
                }
            }
        }
    }

    /// Account id and proper name of a living character, by name.
    pub fn find_character(&self, name: &str) -> Option<(u32, String)> {
        self.store.accounts.iter().find_map(|a| {
            a.characters
                .iter()
                .find(|c| !c.deleted && c.name.eq_ignore_ascii_case(name))
                .map(|c| (a.id, c.name.clone()))
        })
    }

    /// Every live character on every account, for the ranking board. Zircon
    /// ranks offline characters too, so this is the whole roster.
    pub fn all_characters(&self) -> impl Iterator<Item = &CharacterRecord> {
        self.store
            .accounts
            .iter()
            .flat_map(|a| a.characters.iter())
            .filter(|c| !c.deleted)
    }

    /// Like `find_character`, but also gives the character id, which the
    /// friend list keys on (Zircon friends a character, blocks an account).
    pub fn find_character_full(&self, name: &str) -> Option<(u32, u32, String)> {
        self.store.accounts.iter().find_map(|a| {
            a.characters
                .iter()
                .find(|c| !c.deleted && c.name.eq_ignore_ascii_case(name))
                .map(|c| (a.id, c.id, c.name.clone()))
        })
    }

    pub fn character(&self, account: u32, id: u32) -> Option<&CharacterRecord> {
        self.account(account)?
            .characters
            .iter()
            .find(|c| c.id == id && !c.deleted)
    }

    pub fn character_mut(&mut self, account: u32, id: u32) -> Option<&mut CharacterRecord> {
        self.account_mut(account)?
            .characters
            .iter_mut()
            .find(|c| c.id == id && !c.deleted)
    }
}

/// A fresh level-1 warrior record, for `--inspect` and tests.
pub fn test_character(name: &str) -> CharacterRecord {
    CharacterRecord {
        id: 0,
        name: name.to_string(),
        class: mir_proto::Class::Warrior,
        gender: mir_proto::Gender::Male,
        hair: 1,
        level: 1,
        experience: 0,
        experience_fraction: 0.0,
        hp: 0,
        mp: 0,
        map: String::new(),
        location: mir_proto::Point::default(),
        direction: mir_proto::Direction::Down,
        created: 0,
        last_login: 0,
        deleted: false,
        gm: false,
        caption: None,
        items: Vec::new(),
        inventory_size: mir_proto::INVENTORY_SIZE as u8,
        gold: 0,
        event_rewards: Vec::new(),
        next_item_id: 0,
        magics: Vec::new(),
        belt: Vec::new(),
        currencies: Vec::new(),
        rebirth: 0,
        quests: Vec::new(),
        allow_group: false,
        attack_mode: 0,
        pet_mode: 0,
        pk_points: 0,
        fame: 0,
        hermit_stats: Vec::new(),
        horse: 0,
        partner: 0,
        partner_name: String::new(),
        wedding_ring: 0,
        refines: Vec::new(),
        companions: Vec::new(),
        active_companion: None,
        companion_unlocks: Vec::new(),
        hair_colour: 0,
        wardrobe_item: None,
        wardrobe_helmet: None,
        wardrobe_weapon: None,
        wardrobe_shoes: None,
        discipline: 0,
        discipline_experience: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_lifecycle() {
        let dir = std::env::temp_dir().join(format!("zircon-accounts-{}", std::process::id()));
        let path = dir.join("accounts.json");
        let mut a = Accounts::load(&path).unwrap();
        assert!(a.create("bob", "short").is_err());
        let id = a.create("bob@example.com", "secret123").unwrap();
        assert!(a.create("BOB@example.com", "secret123").is_err());
        assert!(a.login("bob@example.com", "wrong").is_err());
        assert_eq!(a.login("Bob@example.com", "secret123").unwrap(), id);
        let c = a
            .new_character(id, "Hero", Class::Warrior, Gender::Male, 1)
            .unwrap();
        assert!(a
            .new_character(id, "hero", Class::Wizard, Gender::Female, 2)
            .is_err());
        assert!(a
            .new_character(id, "1bad", Class::Wizard, Gender::Female, 2)
            .is_err());
        assert_eq!(a.summaries(id).len(), 1);
        a.character_mut(id, c.id).unwrap().pet_mode = mir_proto::pet_mode::PVP;
        a.character_mut(id, c.id).unwrap().level = MAX_PLAYER_LEVEL;
        a.mark_dirty();
        a.save_if_dirty().unwrap();
        let b = Accounts::load(&path).unwrap();
        assert_eq!(b.summaries(id)[0].name, "Hero");
        assert_eq!(b.summaries(id)[0].level, MAX_PLAYER_LEVEL);
        assert_eq!(
            b.character(id, c.id).unwrap().pet_mode,
            mir_proto::pet_mode::PVP
        );
        let mut b = b;
        b.delete_character(id, c.id).unwrap();
        assert!(b.summaries(id).is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn gm_grant_and_ban_survive_reload() {
        let dir = std::env::temp_dir().join(format!("zircon-gm-accounts-{}", std::process::id()));
        let path = dir.join("accounts.json");
        let mut accounts = Accounts::load(&path).unwrap();
        let id = accounts.create("gm@example.test", "secret123").unwrap();
        let character = accounts
            .new_character(id, "Master", Class::Warrior, Gender::Male, 1)
            .unwrap();
        assert!(!character.gm);
        accounts.character_mut(id, character.id).unwrap().gm = true;
        accounts.account_mut(id).unwrap().banned_until = now_secs() + 3600;
        accounts.mark_dirty();
        accounts.save_if_dirty().unwrap();
        let mut loaded = Accounts::load(&path).unwrap();
        assert!(loaded.character(id, character.id).unwrap().gm);
        assert_eq!(
            loaded.login("gm@example.test", "secret123"),
            Err("Account banned".into())
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}

//! Mail (Zircon `PlayerObject.Quests` mail section): send items and gold to
//! a character by name (offline is fine), read, take items in a safe zone,
//! delete when empty. Boxes are per account in `mail.json` under the data
//! dir.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::*;
use crate::items::UserItem;
use mir_proto::{ChatKind, MailSummary};

/// Zircon: 5 item links per mail, 50 stored items per account, 10 s between
/// sends, subject 30 and message 300 characters.
pub const MAX_MAIL_ITEMS: usize = 5;
pub const MAX_MAIL_STORAGE: usize = 50;
pub const MAIL_DELAY: u64 = 10_000;
/// Pseudo slot for the gold attached to a mail.
pub const GOLD_SLOT: u8 = 255;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mail {
    pub index: u32,
    pub sender: String,
    /// Unix seconds.
    pub date: u64,
    pub subject: String,
    pub message: String,
    pub opened: bool,
    pub gold: u64,
    pub items: Vec<UserItem>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct MailStore {
    pub next_index: u32,
    /// Account id -> mails.
    pub boxes: HashMap<u32, Vec<Mail>>,
    #[serde(skip)]
    path: Option<PathBuf>,
}

impl MailStore {
    pub(super) fn load(path: PathBuf) -> MailStore {
        let mut store: MailStore = super::load_json(&path);
        store.path = Some(path);
        store
    }

    pub(super) fn save(&self) {
        if let Some(p) = &self.path {
            super::save_json(p, self);
        }
    }
}

pub(super) fn summary(m: &Mail) -> MailSummary {
    MailSummary {
        index: m.index,
        opened: m.opened,
        date: m.date,
        sender: m.sender.clone(),
        subject: m.subject.clone(),
        message: m.message.clone(),
        gold: m.gold,
        items: m.items.iter().map(|i| i.instance()).collect(),
    }
}

impl World {
    fn mail_line(&mut self, id: ObjectId, text: String) {
        self.send_to(
            id,
            ServerMessage::Say {
                id: None,
                kind: ChatKind::System,
                text,
            },
        );
    }

    pub(super) fn account_of(&self, id: ObjectId) -> Option<u32> {
        self.objects.get(&id)?.player().map(|p| p.account)
    }

    /// Online player of an account (any character).
    pub(super) fn online_account(&self, account: u32) -> Option<ObjectId> {
        self.players()
            .find(|o| o.player().is_some_and(|p| p.account == account))
            .map(|o| o.id)
    }

    /// Send the mailbox on entry.
    pub fn mail_login(&mut self, id: ObjectId) {
        let Some(account) = self.account_of(id) else {
            return;
        };
        let list: Vec<MailSummary> = self
            .mail_store
            .boxes
            .get(&account)
            .map(|b| b.iter().map(summary).collect())
            .unwrap_or_default();
        self.send_to(id, ServerMessage::MailList(list));
    }

    /// Send a mail; `recipient` is the account the name resolved to (the
    /// caller looks it up so offline characters can receive mail).
    #[allow(clippy::too_many_arguments)]
    pub fn mail_send(
        &mut self,
        id: ObjectId,
        recipient: Option<(u32, String)>,
        subject: String,
        message: String,
        gold: u64,
        items: Vec<(Grid, u8, u32)>,
    ) {
        let Some(o) = self.objects.get(&id) else {
            return;
        };
        let Some(p) = o.player() else {
            return;
        };
        if self.now < p.mail_time {
            return;
        }
        let Some((account, rname)) = recipient else {
            self.mail_line(id, "Could not find that character.".into());
            return;
        };
        if account == p.account {
            self.mail_line(id, "You cannot mail yourself.".into());
            return;
        }
        if items.len() > MAX_MAIL_ITEMS
            || subject.chars().count() > 30
            || message.chars().count() > 300
        {
            return;
        }
        if gold > p.bag.gold {
            self.mail_line(id, "You do not have that much gold.".into());
            return;
        }
        if !items.is_empty() && !o.in_safe_zone {
            self.mail_line(id, "You can only mail items from a safe zone.".into());
            return;
        }
        let stored = self
            .mail_store
            .boxes
            .get(&account)
            .map(|b| {
                b.iter()
                    .map(|m| m.items.len() + usize::from(m.gold > 0))
                    .sum()
            })
            .unwrap_or(0);
        if (!items.is_empty() || gold > 0) && stored >= MAX_MAIL_STORAGE {
            self.mail_line(id, format!("{rname}'s mailbox is full."));
            return;
        }
        if !super::refine::distinct_cells(&items) {
            return;
        }
        // Validate every attached cell before taking anything.
        let mut attached: Vec<UserItem> = Vec::new();
        for (grid, slot, count) in &items {
            if *grid == Grid::Storage {
                return;
            }
            let Some(it) = p.bag.grid(*grid).get(*slot as usize).cloned().flatten() else {
                self.mail_line(id, "Nothing there.".into());
                return;
            };
            if *count == 0 || *count > it.count {
                return;
            }
            if it.bound || self.data.items.get(&it.info).is_some_and(|d| !d.can_trade) {
                self.mail_line(id, "That item cannot be mailed.".into());
                return;
            }
            attached.push(UserItem {
                count: *count,
                ..it
            });
        }
        let sender = p.name.clone();
        let now = self.now;
        let p = self
            .objects
            .get_mut(&id)
            .and_then(|o| o.player_mut())
            .unwrap();
        p.mail_time = now + MAIL_DELAY;
        p.bag.gold -= gold;
        let mut changes = Changed::new();
        for (grid, slot, count) in &items {
            if let Some(ch) = p.bag.take(*grid, *slot, *count) {
                changes.push(ch);
            }
        }
        self.send_changes(id, changes);
        self.send_gold(id);
        self.account_dirty = true;
        let mail = {
            let store = &mut self.mail_store;
            store.next_index += 1;
            let mail = Mail {
                index: store.next_index,
                sender,
                date: crate::accounts::now_secs(),
                subject,
                message,
                opened: false,
                gold,
                items: attached,
            };
            store.boxes.entry(account).or_default().push(mail.clone());
            store.save();
            mail
        };
        self.mail_line(id, format!("Mail sent to {rname}."));
        if let Some(target) = self.online_account(account) {
            self.send_to(target, ServerMessage::MailNew(summary(&mail)));
        }
    }

    pub fn mail_opened(&mut self, id: ObjectId, index: u32) {
        let Some(account) = self.account_of(id) else {
            return;
        };
        if let Some(m) = self
            .mail_store
            .boxes
            .get_mut(&account)
            .and_then(|b| b.iter_mut().find(|m| m.index == index))
        {
            if !m.opened {
                m.opened = true;
                self.mail_store.save();
            }
        }
    }

    /// Take one attachment (`GOLD_SLOT` for the gold); needs a safe zone
    /// and bag room.
    pub fn mail_get_item(&mut self, id: ObjectId, index: u32, slot: u8) {
        let Some(account) = self.account_of(id) else {
            return;
        };
        let o = &self.objects[&id];
        if !o.in_safe_zone {
            self.mail_line(id, "You can only take mail items in a safe zone.".into());
            return;
        }
        let Some(mail) = self
            .mail_store
            .boxes
            .get(&account)
            .and_then(|b| b.iter().find(|m| m.index == index))
            .cloned()
        else {
            self.send_to(id, ServerMessage::MailDelete { index });
            return;
        };
        if slot == GOLD_SLOT {
            if mail.gold == 0 {
                return;
            }
            let p = self
                .objects
                .get_mut(&id)
                .and_then(|o| o.player_mut())
                .unwrap();
            p.bag.gold += mail.gold;
            self.send_gold(id);
            self.account_dirty = true;
        } else {
            let Some(item) = mail.items.get(slot as usize).cloned() else {
                self.send_to(id, ServerMessage::MailItemDelete { index, slot });
                return;
            };
            let p = o.player().unwrap();
            if !p.bag.can_gain_item(&self.data, &item, i32::MAX) {
                self.mail_line(id, "You need more bag space.".into());
                return;
            }
            let p = self
                .objects
                .get_mut(&id)
                .and_then(|o| o.player_mut())
                .unwrap();
            let mut next = p.next_item_id;
            let changes = p.bag.gain_item(&self.data, item.clone(), &mut next);
            p.next_item_id = next;
            self.send_changes(id, changes);
            self.account_dirty = true;
        }
        if let Some(m) = self
            .mail_store
            .boxes
            .get_mut(&account)
            .and_then(|b| b.iter_mut().find(|m| m.index == index))
        {
            if slot == GOLD_SLOT {
                m.gold = 0;
            } else {
                m.items.remove(slot as usize);
            }
        }
        self.mail_store.save();
        self.send_to(id, ServerMessage::MailItemDelete { index, slot });
    }

    pub fn mail_delete(&mut self, id: ObjectId, index: u32) {
        let Some(account) = self.account_of(id) else {
            return;
        };
        let Some(b) = self.mail_store.boxes.get_mut(&account) else {
            return;
        };
        match b.iter().position(|m| m.index == index) {
            Some(pos) if b[pos].items.is_empty() && b[pos].gold == 0 => {
                b.remove(pos);
                self.mail_store.save();
                self.send_to(id, ServerMessage::MailDelete { index });
            }
            Some(_) => self.mail_line(id, "Take the items out of the mail first.".into()),
            None => self.send_to(id, ServerMessage::MailDelete { index }),
        }
    }
}

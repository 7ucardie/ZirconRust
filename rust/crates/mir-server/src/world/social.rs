//! Friends and blocking (Zircon `CommunicationDialog` plus the
//! `FriendAdd`/`BlockAdd` handlers in `SConnection`).
//!
//! Zircon splits the two: a friend is stored per **character**
//! (`Character.Friends`), while a block is stored per **account**
//! (`Account.BlockingList`) and names the blocked account, so blocking one
//! character blocks every character that person owns. Blocking is mutual --
//! `SEnvir.IsBlocking` returns true when either side blocks the other --
//! and a blocked whisper is answered with "could not find", so the sender
//! is never told they were blocked.
//!
//! Persisted in `social.json` under the data dir.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::*;
use mir_proto::{online_state, BlockSummary, ChatKind, FriendSummary};

/// One entry of a character's friend list (Zircon `FriendInfo`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Friend {
    pub index: u32,
    /// Character that owns this list.
    pub character: u32,
    /// The befriended character.
    pub friend_character: u32,
    pub friend_name: String,
}

/// One entry of an account's block list (Zircon `BlockInfo`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    pub index: u32,
    /// Account that owns this list.
    pub account: u32,
    /// Zircon blocks the account, not the character.
    pub blocked_account: u32,
    /// The name it was added under, which is what the list shows.
    pub blocked_name: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct SocialStore {
    pub next_index: u32,
    pub friends: Vec<Friend>,
    pub blocks: Vec<Block>,
    #[serde(skip)]
    path: Option<PathBuf>,
}

impl SocialStore {
    pub fn load(path: PathBuf) -> SocialStore {
        let mut store: SocialStore = super::load_json(&path);
        store.path = Some(path);
        store
    }

    fn save(&self) {
        if let Some(p) = &self.path {
            super::save_json(p, self);
        }
    }

    fn next(&mut self) -> u32 {
        self.next_index += 1;
        self.next_index
    }

    /// A character's friends, in insertion order.
    pub fn friends_of(&self, character: u32) -> impl Iterator<Item = &Friend> {
        self.friends
            .iter()
            .filter(move |f| f.character == character)
    }

    /// Characters that have `character` on their list (Zircon
    /// `Character.FriendedBy`), which is who hears about a state change.
    pub fn friended_by(&self, character: u32) -> impl Iterator<Item = &Friend> {
        self.friends
            .iter()
            .filter(move |f| f.friend_character == character)
    }

    /// An account's blocks, in insertion order.
    pub fn blocks_of(&self, account: u32) -> impl Iterator<Item = &Block> {
        self.blocks.iter().filter(move |b| b.account == account)
    }

    /// Zircon `SEnvir.IsBlocking`: true when **either** account blocks the
    /// other, so blocking someone also stops them reaching you.
    pub fn is_blocking(&self, a: u32, b: u32) -> bool {
        if a == b {
            return false;
        }
        self.blocks.iter().any(|x| {
            (x.account == a && x.blocked_account == b) || (x.account == b && x.blocked_account == a)
        })
    }
}

impl World {
    fn social_line(&mut self, id: ObjectId, text: String) {
        self.send_to(
            id,
            ServerMessage::Say {
                id: None,
                kind: ChatKind::System,
                text,
            },
        );
    }

    /// Whether these two accounts may reach each other at all.
    pub fn is_blocking(&self, a: u32, b: u32) -> bool {
        self.social_store.is_blocking(a, b)
    }

    /// The state a character is advertising: `OFFLINE` unless they are
    /// logged in, and then whatever they last chose.
    fn state_of_character(&self, character: u32) -> u8 {
        if let Some(state) = self.dev_states.get(&character) {
            return *state;
        }
        self.players()
            .find(|o| o.player().is_some_and(|p| p.character == character))
            .and_then(|o| o.player())
            .map(|p| p.online_state)
            .unwrap_or(online_state::OFFLINE)
    }

    /// The friend list as the owner sees it, sorted by state the way
    /// Zircon's `RefreshFriendList` orders it.
    pub fn friend_list(&self, character: u32) -> Vec<FriendSummary> {
        let mut out: Vec<FriendSummary> = self
            .social_store
            .friends_of(character)
            .map(|f| FriendSummary {
                index: f.index,
                name: f.friend_name.clone(),
                state: self.state_of_character(f.friend_character),
            })
            .collect();
        out.sort_by_key(|f| (f.state, f.index));
        out
    }

    pub fn block_list(&self, account: u32) -> Vec<BlockSummary> {
        self.social_store
            .blocks_of(account)
            .map(|b| BlockSummary {
                index: b.index,
                name: b.blocked_name.clone(),
            })
            .collect()
    }

    /// `ZIRCON_DEV_FRIENDS=1`: give a fresh character one friend in each
    /// state and one blocked name, so the window can be screenshotted
    /// without a second account. The rows are real store entries; only
    /// their states are faked, through `dev_states`.
    fn dev_seed(&mut self, id: ObjectId) {
        if std::env::var_os("ZIRCON_DEV_FRIENDS").is_none() {
            return;
        }
        let Some(p) = self.objects.get(&id).and_then(|o| o.player()) else {
            return;
        };
        let (character, account) = (p.character, p.account);
        if self.social_store.friends_of(character).next().is_some() {
            return;
        }
        // Character ids far above any real one, so nothing collides.
        for (offset, (name, state)) in [
            ("Isolde", online_state::ONLINE),
            ("Tarrant", online_state::AWAY),
            ("Mirren", online_state::BUSY),
            ("Calloway", online_state::OFFLINE),
        ]
        .into_iter()
        .enumerate()
        {
            let friend_character = 900_000 + offset as u32;
            let index = self.social_store.next();
            self.social_store.friends.push(Friend {
                index,
                character,
                friend_character,
                friend_name: name.into(),
            });
            self.dev_states.insert(friend_character, state);
        }
        let index = self.social_store.next();
        self.social_store.blocks.push(Block {
            index,
            account,
            blocked_account: 900_100,
            blocked_name: "Grubb".into(),
        });
    }

    /// Both lists and the advertised state, sent on entry.
    pub fn send_social(&mut self, id: ObjectId) {
        self.dev_seed(id);
        let Some(p) = self.objects.get(&id).and_then(|o| o.player()) else {
            return;
        };
        let (character, account, state) = (p.character, p.account, p.online_state);
        let friends = self.friend_list(character);
        let blocks = self.block_list(account);
        self.send_to(id, ServerMessage::Friends(friends));
        self.send_to(id, ServerMessage::Blocks(blocks));
        self.send_to(id, ServerMessage::OnlineState { state });
    }

    fn send_friends(&mut self, id: ObjectId) {
        let Some(p) = self.objects.get(&id).and_then(|o| o.player()) else {
            return;
        };
        let friends = self.friend_list(p.character);
        self.send_to(id, ServerMessage::Friends(friends));
    }

    fn send_blocks(&mut self, id: ObjectId) {
        let Some(p) = self.objects.get(&id).and_then(|o| o.player()) else {
            return;
        };
        let blocks = self.block_list(p.account);
        self.send_to(id, ServerMessage::Blocks(blocks));
    }

    /// Befriend a character. `resolved` is `(account, character, name)` as
    /// the account registry spells it, or `None` when the name is unknown.
    pub fn friend_add(
        &mut self,
        id: ObjectId,
        resolved: Option<(u32, u32, String)>,
        typed: String,
    ) {
        let Some(p) = self.objects.get(&id).and_then(|o| o.player()) else {
            return;
        };
        let (character, my_name) = (p.character, p.name.clone());
        let Some((_, friend_character, name)) = resolved else {
            self.social_line(id, format!("Could not find {typed}."));
            return;
        };
        if friend_character == character {
            self.social_line(id, "You cannot befriend yourself.".into());
            return;
        }
        if self
            .social_store
            .friends_of(character)
            .any(|f| f.friend_character == friend_character)
        {
            self.social_line(id, format!("{name} is already your friend."));
            return;
        }
        let index = self.social_store.next();
        self.social_store.friends.push(Friend {
            index,
            character,
            friend_character,
            friend_name: name.clone(),
        });
        self.social_store.save();
        let _ = my_name;
        self.social_line(id, format!("{name} added to your friend list."));
        self.send_friends(id);
    }

    pub fn friend_remove(&mut self, id: ObjectId, index: u32) {
        let Some(p) = self.objects.get(&id).and_then(|o| o.player()) else {
            return;
        };
        let character = p.character;
        let before = self.social_store.friends.len();
        self.social_store
            .friends
            .retain(|f| !(f.character == character && f.index == index));
        if self.social_store.friends.len() == before {
            return;
        }
        self.social_store.save();
        self.send_friends(id);
    }

    /// Block the account behind a character name.
    pub fn block_add(&mut self, id: ObjectId, resolved: Option<(u32, u32, String)>, typed: String) {
        let Some(p) = self.objects.get(&id).and_then(|o| o.player()) else {
            return;
        };
        let account = p.account;
        let Some((blocked_account, _, name)) = resolved else {
            self.social_line(id, format!("Could not find {typed}."));
            return;
        };
        if blocked_account == account {
            self.social_line(id, "You cannot block yourself.".into());
            return;
        }
        if self
            .social_store
            .blocks_of(account)
            .any(|b| b.blocked_account == blocked_account)
        {
            self.social_line(id, format!("{name} is already blocked."));
            return;
        }
        let index = self.social_store.next();
        self.social_store.blocks.push(Block {
            index,
            account,
            blocked_account,
            blocked_name: name.clone(),
        });
        self.social_store.save();
        self.social_line(id, format!("{name} blocked."));
        self.send_blocks(id);
    }

    pub fn block_remove(&mut self, id: ObjectId, index: u32) {
        let Some(p) = self.objects.get(&id).and_then(|o| o.player()) else {
            return;
        };
        let account = p.account;
        let before = self.social_store.blocks.len();
        self.social_store
            .blocks
            .retain(|b| !(b.account == account && b.index == index));
        if self.social_store.blocks.len() == before {
            return;
        }
        self.social_store.save();
        self.send_blocks(id);
    }

    /// Zircon `C.ChangeOnlineState`: advertise a different state, then tell
    /// everyone who has us on their list.
    pub fn change_online_state(&mut self, id: ObjectId, state: u8) {
        if !online_state::ALL.contains(&state) {
            return;
        }
        let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) else {
            return;
        };
        if p.online_state == state {
            return;
        }
        p.online_state = state;
        self.send_to(id, ServerMessage::OnlineState { state });
        self.update_online_state(id, false);
    }

    /// Zircon `PlayerObject.UpdateOnlineState`: push this character's state
    /// to every player who friended them. With `announce`, the ones who see
    /// them come Online also get a chat line, which is what login does.
    pub fn update_online_state(&mut self, id: ObjectId, announce: bool) {
        let Some(p) = self.objects.get(&id).and_then(|o| o.player()) else {
            return;
        };
        let (character, name, state) = (p.character, p.name.clone(), p.online_state);
        // Who has this character on their list, and what index they know it by.
        let watchers: Vec<(u32, u32)> = self
            .social_store
            .friended_by(character)
            .map(|f| (f.character, f.index))
            .collect();
        if watchers.is_empty() {
            return;
        }
        for (owner_character, index) in watchers {
            let Some(target) = self
                .players()
                .find(|o| o.player().is_some_and(|p| p.character == owner_character))
                .map(|o| o.id)
            else {
                continue;
            };
            self.send_to(
                target,
                ServerMessage::FriendUpdate(FriendSummary {
                    index,
                    name: name.clone(),
                    state,
                }),
            );
            if announce && state == online_state::ONLINE {
                self.social_line(
                    target,
                    format!("{name} is now {}.", online_state::name(state)),
                );
            }
        }
    }

    /// Called as a character leaves the world: their friends see Offline.
    /// The player object is still present, so its state is forced first.
    pub fn go_offline(&mut self, id: ObjectId) {
        if let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) {
            p.online_state = online_state::OFFLINE;
        }
        self.update_online_state(id, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> SocialStore {
        SocialStore::default()
    }

    #[test]
    fn blocking_is_mutual() {
        let mut s = store();
        s.blocks.push(Block {
            index: 1,
            account: 10,
            blocked_account: 20,
            blocked_name: "Bob".into(),
        });
        // Either direction counts, exactly as Zircon's IsBlocking does.
        assert!(s.is_blocking(10, 20));
        assert!(s.is_blocking(20, 10));
        assert!(!s.is_blocking(10, 30));
        // An account never blocks itself.
        assert!(!s.is_blocking(10, 10));
    }

    #[test]
    fn lists_are_scoped_to_their_owner() {
        let mut s = store();
        s.friends.push(Friend {
            index: 1,
            character: 1,
            friend_character: 2,
            friend_name: "Bob".into(),
        });
        s.friends.push(Friend {
            index: 2,
            character: 3,
            friend_character: 2,
            friend_name: "Bob".into(),
        });
        assert_eq!(s.friends_of(1).count(), 1);
        assert_eq!(s.friends_of(3).count(), 1);
        assert_eq!(s.friends_of(2).count(), 0);
        // Both owners hear when Bob's state changes.
        assert_eq!(s.friended_by(2).count(), 2);
    }
}

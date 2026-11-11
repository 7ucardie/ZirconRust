//! Character editing: the four System items that restyle, re-sex, dye or
//! rename a character (Zircon `EditCharacterDialog` and the `GenderChange` /
//! `HairChange` / `ArmourDye` / `NameChange` handlers in
//! `PlayerObject.Inventory.cs`).
//!
//! Each change spends one matching item out of the bag and then broadcasts
//! the new look, because Zircon's `SendChangeUpdate` broadcasts rather than
//! replying: everyone who can see the character sees the change at once.

use mir_proto::{slot, ChatKind, Class, Gender, ObjectId, ServerMessage};

use super::World;

/// Zircon `ItemEffect` values carried by the four change items. The pack
/// ships one item for each: Gender Change, Hair Change, Armour Dye and
/// Name Change.
pub const ITEM_EFFECT_GENDER_CHANGE: u8 = 50;
pub const ITEM_EFFECT_HAIR_CHANGE: u8 = 51;
pub const ITEM_EFFECT_ARMOUR_DYE: u8 = 52;
pub const ITEM_EFFECT_NAME_CHANGE: u8 = 53;

/// Zircon `Globals.MinCharacterNameLength` / `MaxCharacterNameLength`, the
/// bounds of `CharacterReg` (`^[A-Za-z0-9]{3,15}$`).
pub const MIN_NAME: usize = 3;
pub const MAX_NAME: usize = 15;

/// Zircon `Globals.CharacterReg`: letters and digits only, 3 to 15 of them.
pub fn name_acceptable(name: &str) -> bool {
    (MIN_NAME..=MAX_NAME).contains(&name.chars().count())
        && name.chars().all(|c| c.is_ascii_alphanumeric())
}

/// The highest hair style a class and gender may wear. Zircon caps warriors,
/// wizards and taoists at 10 for men and 11 for women, and assassins at 5
/// whichever way round.
pub fn max_hair(class: Class, gender: Gender) -> u8 {
    match class {
        Class::Assassin => 5,
        _ => match gender {
            Gender::Male => 10,
            Gender::Female => 11,
        },
    }
}

/// Zircon pairs hair style 0 with a blank colour and any other style with an
/// opaque one, and refuses the mismatch outright.
fn hair_colour_agrees(hair: u8, colour: u32) -> bool {
    if hair == 0 {
        colour == 0
    } else {
        colour >> 24 == 0xFF
    }
}

impl World {
    /// Zircon `HairChange`: restyle, spending a Hair Change item.
    pub fn hair_change(&mut self, id: ObjectId, hair: u8, colour: u32) {
        let Some(p) = self.objects.get(&id).and_then(|o| o.player()) else {
            return;
        };
        if hair > max_hair(p.class, p.gender) || !hair_colour_agrees(hair, colour) {
            return;
        }
        if !self.spend_change_item(id, ITEM_EFFECT_HAIR_CHANGE) {
            return;
        }
        if let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) {
            p.hair = hair;
            p.hair_colour = colour;
        }
        self.refresh_appearance(id);
        self.say_change(id, "Your hair has changed.".into());
    }

    /// Zircon `GenderChange`: swap gender and restyle together, spending a
    /// Gender Change item. Refused while armour is worn, because the body
    /// sprite underneath it would change.
    pub fn gender_change(&mut self, id: ObjectId, gender: Gender, hair: u8, colour: u32) {
        let Some(p) = self.objects.get(&id).and_then(|o| o.player()) else {
            return;
        };
        if p.gender == gender {
            return;
        }
        if !hair_colour_agrees(hair, colour) || hair > max_hair(p.class, gender) {
            return;
        }
        if p.bag
            .equipment
            .get(slot::ARMOUR)
            .is_some_and(|i| i.is_some())
        {
            self.say_change(id, "Take your armour off first.".into());
            return;
        }
        if !self.spend_change_item(id, ITEM_EFFECT_GENDER_CHANGE) {
            return;
        }
        if let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) {
            p.gender = gender;
            p.hair = hair;
            p.hair_colour = colour;
        }
        self.refresh_appearance(id);
        self.say_change(id, "You look different.".into());
    }

    /// Zircon `ArmourDye`: dye the worn armour, spending an Armour Dye item.
    ///
    /// Assassins are refused. Zircon's assassin branch reads
    /// `if (colour.ToArgb() != 0) return; return;` -- the second `return` is
    /// unconditional, so no assassin ever dyes anything. We follow the code,
    /// not the intent, as we do elsewhere.
    pub fn armour_dye(&mut self, id: ObjectId, colour: u32) {
        let Some(p) = self.objects.get(&id).and_then(|o| o.player()) else {
            return;
        };
        if p.bag
            .equipment
            .get(slot::ARMOUR)
            .is_none_or(|i| i.is_none())
        {
            self.say_change(id, "Wear the armour you want dyed.".into());
            return;
        }
        if p.class == Class::Assassin {
            self.say_change(id, "Assassins cannot dye armour.".into());
            return;
        }
        if colour >> 24 != 0xFF {
            return;
        }
        if !self.spend_change_item(id, ITEM_EFFECT_ARMOUR_DYE) {
            return;
        }
        if let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) {
            if let Some(Some(armour)) = p.bag.equipment.get_mut(slot::ARMOUR) {
                armour.colour = colour;
            }
        }
        // Push the dyed item back so the bag grid repaints, then the look.
        let changed = self
            .objects
            .get(&id)
            .and_then(|o| o.player())
            .and_then(|p| p.bag.equipment.get(slot::ARMOUR))
            .and_then(|i| i.as_ref())
            .map(|i| i.instance());
        self.send_changes(
            id,
            vec![(mir_proto::Grid::Equipment, slot::ARMOUR as u8, changed)],
        );
        self.refresh_appearance(id);
        self.say_change(id, "Your armour has been dyed.".into());
    }

    /// Zircon `NameChange`: rename, spending a Name Change item.
    ///
    /// `holder` is whoever already answers to that name, as
    /// `(account, character, name)`. Zircon only refuses when the holder is
    /// on a *different* account, so a player may reuse a name they already
    /// own on another character.
    pub fn name_change(
        &mut self,
        id: ObjectId,
        new_name: String,
        holder: Option<(u32, u32, String)>,
    ) {
        let Some(p) = self.objects.get(&id).and_then(|o| o.player()) else {
            return;
        };
        if !name_acceptable(&new_name) {
            self.say_change(id, "Unacceptable character name.".into());
            return;
        }
        if p.name == new_name {
            self.say_change(id, format!("Your name is already {new_name}."));
            return;
        }
        if holder.is_some_and(|(account, _, _)| account != p.account) {
            self.say_change(id, "This name is already in use.".into());
            return;
        }
        if !self.spend_change_item(id, ITEM_EFFECT_NAME_CHANGE) {
            return;
        }
        if let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) {
            p.name = new_name.clone();
        }
        self.refresh_appearance(id);
        self.say_change(id, format!("You are now known as {new_name}."));
    }

    /// The `ItemInfo` index of the change item for one effect, if the data
    /// carries one.
    pub fn change_item(&self, effect: u8) -> Option<i32> {
        self.data
            .items
            .values()
            .find(|i| i.effect == effect)
            .map(|i| i.index)
    }

    /// Take one change item of this effect out of the bag, telling the
    /// player when they have none. Zircon silently refuses; we say why,
    /// because the window offers the change before checking.
    fn spend_change_item(&mut self, id: ObjectId, effect: u8) -> bool {
        let Some(o) = self.objects.get(&id) else {
            return false;
        };
        let Some(p) = o.player() else {
            return false;
        };
        if o.dead || p.mounted {
            return false;
        }
        let slot = p.bag.inventory.iter().position(|cell| {
            cell.as_ref().is_some_and(|item| {
                item.expires_at.is_none_or(|t| t > super::store::now_secs())
                    && self.data.items.get(&item.info).is_some_and(|d| {
                        d.effect == effect
                            && crate::items::can_use(d, p.class, p.gender, p.level).is_ok()
                    })
            })
        });
        let Some(slot) = slot else {
            let name = self
                .change_item(effect)
                .and_then(|info| self.data.items.get(&info))
                .map(|d| d.name.as_str())
                .unwrap_or("change item");
            self.say_change(id, format!("You need a usable {name}."));
            return false;
        };
        let change = self
            .objects
            .get_mut(&id)
            .unwrap()
            .player_mut()
            .unwrap()
            .bag
            .take(mir_proto::Grid::Inventory, slot as u8, 1);
        self.send_changes(id, change.into_iter().collect());
        true
    }

    fn say_change(&mut self, id: ObjectId, text: String) {
        self.send_to(
            id,
            ServerMessage::Say {
                id: None,
                kind: ChatKind::System,
                text,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_match_zircons_regex() {
        // ^[A-Za-z0-9]{3,15}$
        assert!(name_acceptable("Abc"));
        assert!(name_acceptable("Player123"));
        assert!(name_acceptable("123456789012345"));
        assert!(!name_acceptable("ab"), "under three");
        assert!(!name_acceptable("1234567890123456"), "over fifteen");
        assert!(!name_acceptable("has space"));
        assert!(!name_acceptable("punct!"));
        assert!(!name_acceptable(""));
    }

    #[test]
    fn hair_caps_follow_class_and_gender() {
        assert_eq!(max_hair(Class::Warrior, Gender::Male), 10);
        assert_eq!(max_hair(Class::Warrior, Gender::Female), 11);
        assert_eq!(max_hair(Class::Wizard, Gender::Female), 11);
        assert_eq!(max_hair(Class::Taoist, Gender::Male), 10);
        // Assassins are capped at 5 either way round.
        assert_eq!(max_hair(Class::Assassin, Gender::Male), 5);
        assert_eq!(max_hair(Class::Assassin, Gender::Female), 5);
    }

    #[test]
    fn bald_takes_no_colour_and_hair_must_be_opaque() {
        assert!(hair_colour_agrees(0, 0));
        assert!(!hair_colour_agrees(0, 0xFF00_0000));
        assert!(hair_colour_agrees(3, 0xFF12_3456));
        assert!(!hair_colour_agrees(3, 0x0012_3456), "translucent refused");
        assert!(!hair_colour_agrees(3, 0), "blank refused for real hair");
    }
}

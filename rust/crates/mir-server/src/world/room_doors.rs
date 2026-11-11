//! Ordinary room doors from private map sidecars; no castle ownership rules.
use super::*;
use mir_formats::room_door::RoomDoors;

#[derive(Debug)]
pub struct RoomDoorRole {
    pub closed: bool,
    pub cut: Vec<Point>,
}

impl World {
    pub(super) fn spawn_room_doors(&mut self, map: i32, layout: &RoomDoors) {
        for door in &layout.doors {
            let [x, y] = door.center;
            let at = Point::new(x, y);
            let id = self.create_monster(door.source_monster, map, at, None, None, 0);
            let object = self.objects.get_mut(&id).unwrap();
            object.direction = Direction::from_index(0);
            object.hp = 1;
            object.max_hp = 1;
            object.appearance = Appearance::Monster {
                name: "Wood Door".into(),
                image: 543,
                owner: None,
                skeleton: false,
            };
            let monster = object.monster_mut().unwrap();
            monster.hidden = false;
            monster.guard = false;
            monster.room_door = Some(RoomDoorRole {
                closed: true,
                cut: (y - 3..y + 5).map(|py| Point::new(x, py)).collect(),
            });
        }
    }

    pub(super) fn room_door_blocks_cell(&self, map: i32, point: Point) -> bool {
        self.on_map(map).any(|object| {
            object
                .monster()
                .and_then(|monster| monster.room_door.as_ref())
                .is_some_and(|door| door.closed && door.cut.contains(&point))
        })
    }

    /// Toggle only from beside the door. A closing cut cannot contain another body.
    pub fn player_toggle_room_door(&mut self, player: ObjectId, target: ObjectId) {
        let Some(actor) = self.objects.get(&player) else {
            return;
        };
        if actor.player().is_none() || actor.dead {
            return;
        }
        let (map, location) = (actor.map, actor.location);
        let Some(door) = self
            .objects
            .get(&target)
            .filter(|object| object.map == map)
            .filter(|object| {
                object
                    .monster()
                    .is_some_and(|monster| monster.room_door.is_some())
            })
        else {
            return;
        };
        if location.distance(door.location) > 2 {
            self.send_to(
                player,
                ServerMessage::Chat {
                    text: "Move closer to the door.".into(),
                },
            );
            return;
        }
        let role = door.monster().unwrap().room_door.as_ref().unwrap();
        let closing = !role.closed;
        if closing
            && self.on_map(map).any(|object| {
                object.id != target && object.blocking() && role.cut.contains(&object.location)
            })
        {
            self.send_to(
                player,
                ServerMessage::Chat {
                    text: "The doorway is occupied.".into(),
                },
            );
            return;
        }
        let direction = Direction::from_index(if closing { 0 } else { 7 });
        let door = self.objects.get_mut(&target).unwrap();
        door.monster_mut()
            .unwrap()
            .room_door
            .as_mut()
            .unwrap()
            .closed = closing;
        door.direction = direction;
        self.events.push((
            target,
            ServerMessage::ObjectAttack {
                id: target,
                direction,
                attack_magic: None,
                element: mir_proto::element::NONE,
            },
        ));
    }
}

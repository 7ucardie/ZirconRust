use super::*;

impl Game {
    /// Expire finished effects, fly projectiles, release scheduled payloads.
    pub(super) fn advance_effects(&mut self, now: u64, width: i32, height: i32) {
        let view = View::new(width, height, self.user());
        // Scheduled payloads (after the cast animation).
        let due: Vec<_> = self
            .pending_payloads
            .iter()
            .filter(|p| p.0 <= now)
            .cloned()
            .collect();
        self.pending_payloads.retain(|p| p.0 > now);
        for (_, magic, caster_cell, targets, locations) in due {
            let payload = effects::payload(magic, caster_cell, &targets, &locations, now);
            self.effects.extend(payload.effects);
            let end = sound_table::magic_end(magic);
            if payload.projectiles.is_empty() {
                for &s in end {
                    self.audio.play(s);
                }
            } else {
                for &s in sound_table::magic_travel(magic) {
                    self.audio.play(s);
                }
            }
            for (from, to, mut p) in payload.projectiles {
                p.sound = end.first().copied().unwrap_or(0);
                let (fx, fy) = view.cell_px(from.x, from.y);
                let (tx, ty) = match to {
                    Anchor::Object(id) => match self.objects.get(&id) {
                        Some(o) => view.object_px(o),
                        None => continue,
                    },
                    Anchor::Cell(c) => view.cell_px(c.x, c.y),
                };
                let dist = (((tx - fx) as f32).powi(2) + ((ty - fy) as f32).powi(2)).sqrt();
                p.duration = dist.max(1.0) as u64;
                p.dir16 = effects::direction16((fx as f32, fy as f32), (tx as f32, ty as f32));
                self.projectiles.push(p);
            }
        }
        self.effects.retain(|e| !e.finished(now));
        let mut arrived = Vec::new();
        self.projectiles.retain(|p| {
            if p.progress(now) >= 1.0 {
                arrived.push(p.clone());
                false
            } else {
                true
            }
        });
        for p in arrived {
            self.audio.play(p.sound);
            if let Some((library, start, count, delay, color)) = p.explode {
                self.effects.push(Effect {
                    library,
                    start,
                    count,
                    delay_ms: delay,
                    color,
                    anchor: p.to,
                    direction: None,
                    started: now,
                    blend: Blend::Screen,
                });
            }
        }
    }

    pub(super) fn anchor_px(&self, view: &View, a: Anchor) -> Option<(i32, i32)> {
        match a {
            Anchor::Object(id) => self.objects.get(&id).map(|o| view.object_px(o)),
            Anchor::Cell(c) => Some(view.cell_px(c.x, c.y)),
        }
    }

    pub(super) fn draw_effects(
        &mut self,
        view: &View,
        gpu: &Gpu,
        renderer: &mut SpriteRenderer,
        now: u64,
    ) {
        // Looping Magic Shield rings.
        let shielded: Vec<ObjectId> = self
            .objects
            .values()
            .filter(|o| !o.dead && o.visible_buffs.contains(&buff_type::MAGIC_SHIELD))
            .map(|o| o.id)
            .collect();
        let reflecting: Vec<ObjectId> = self
            .objects
            .values()
            .filter(|o| !o.dead && o.visible_buffs.contains(&buff_type::REFLECT_DAMAGE))
            .map(|o| o.id)
            .collect();
        let celestial: Vec<ObjectId> = self
            .objects
            .values()
            .filter(|o| !o.dead && o.visible_buffs.contains(&buff_type::CELESTIAL_LIGHT))
            .map(|o| o.id)
            .collect();
        let mut effects = self.effects.clone();
        // Reuse the reference teleport burst as a looping exit beacon. The
        // sidecar is visual metadata only; the server still performs travel.
        for portal in &self.portal_markers {
            effects.push(Effect {
                library: effects::MAGIC,
                start: 110 + ((now / 100) % 10) as u32,
                count: 1,
                delay_ms: 1000,
                // Screen blending uses RGB directly, so dim the colour too.
                color: [0.6, 0.6, 0.6, 0.6],
                anchor: Anchor::Cell(Point {
                    x: portal.position.0,
                    y: portal.position.1,
                }),
                direction: None,
                started: now,
                blend: Blend::Screen,
            });
        }
        for id in celestial {
            effects.push(Effect {
                library: effects::MAGIC_EX2,
                start: 300 + ((now / 200) % 3) as u32,
                count: 1,
                delay_ms: 1000,
                color: effects::HOLY,
                anchor: Anchor::Object(id),
                direction: None,
                started: now,
                blend: Blend::Screen,
            });
        }
        for id in reflecting {
            effects.push(Effect {
                library: effects::MAGIC_EX2,
                start: 1240 + ((now / 100) % 3) as u32,
                count: 1,
                delay_ms: 1000,
                color: effects::WHITE,
                anchor: Anchor::Object(id),
                direction: None,
                started: now,
                blend: Blend::Screen,
            });
        }
        for id in shielded {
            effects.push(Effect {
                library: effects::MAGIC,
                start: effects::shield_frame(now),
                count: 1,
                delay_ms: 1000,
                color: effects::WIND,
                anchor: Anchor::Object(id),
                direction: None,
                started: now,
                blend: Blend::Screen,
            });
        }
        for e in effects {
            let Some(frame) = e.frame(now) else { continue };
            let Some((dx, dy)) = self.anchor_px(view, e.anchor) else {
                continue;
            };
            if let Some(info) = self.assets.info(e.library, frame) {
                if let Some(r) = Self::sprite(
                    &mut self.assets,
                    renderer,
                    gpu,
                    e.library,
                    frame,
                    Surface::Image,
                ) {
                    renderer.draw(
                        r,
                        (dx + info.offset_x as i32) as f32,
                        (dy + info.offset_y as i32) as f32,
                        e.color,
                        e.blend,
                    );
                }
            }
        }
        let projectiles = self.projectiles.clone();
        for p in projectiles {
            let (fx, fy) = view.cell_px(p.from.x, p.from.y);
            let Some((tx, ty)) = self.anchor_px(view, p.to) else {
                continue;
            };
            let t = p.progress(now);
            let x = fx as f32 + (tx - fx) as f32 * t;
            let y = fy as f32 + (ty - fy) as f32 * t;
            let frame = p.frame(now);
            if let Some(info) = self.assets.info(p.library, frame) {
                if let Some(r) = Self::sprite(
                    &mut self.assets,
                    renderer,
                    gpu,
                    p.library,
                    frame,
                    Surface::Image,
                ) {
                    renderer.draw(
                        r,
                        x + info.offset_x as f32,
                        y + info.offset_y as f32,
                        p.color,
                        p.blend,
                    );
                }
            }
        }
    }
}

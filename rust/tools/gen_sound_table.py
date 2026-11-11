#!/usr/bin/env python3
"""Generate the sound tables from the C# client source.

Run from repo root:
    python3 rust/tools/gen_sound_table.py > rust/crates/mir-client/src/sound_table.rs

Emits:
- `sound_file(index)`: SoundIndex value -> (file name, channel, loops)
- `monster_sounds(image)`: MonsterImage value -> (attack, struck, die)
- `magic_cast/magic_travel/magic_end/attack_magic(magic)`: MagicType -> sounds
- named constants for the indices the client triggers directly.
"""
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
enum_src = (ROOT / 'LibraryCore' / 'Enum.cs').read_text()
mgr_src = (ROOT / 'Client' / 'Envir' / 'DXSoundManager.cs').read_text()
mon_src = (ROOT / 'Client' / 'Models' / 'MonsterObject.cs').read_text()
map_src = (ROOT / 'Client' / 'Models' / 'MapObject.cs').read_text()


def parse_enum(src, name):
    m = re.search(r'enum ' + name + r'\s*(?::\s*\w+)?\s*\{(.*?)\n    \}', src, re.S)
    body = m.group(1)
    body = re.sub(r'//.*', '', body)
    body = re.sub(r'/\*.*?\*/', '', body, flags=re.S)
    body = re.sub(r'\[[^\]]*\]', '', body)
    body = re.sub(r'#\w+.*', '', body)
    values = {}
    current = 0
    for entry in body.split(','):
        entry = entry.strip()
        if not entry:
            continue
        if '=' in entry:
            n, v = [x.strip() for x in entry.split('=', 1)]
            v = v.replace('<<', '<<')
            current = int(eval(v))
        else:
            n = entry
        values[n] = current
        current += 1
    return values


sound_index = parse_enum(enum_src, 'SoundIndex')
monster_image = parse_enum(enum_src, 'MonsterImage')
magic_type = parse_enum(enum_src, 'MagicType')

channels = {'None': 0, 'System': 1, 'Music': 2, 'Magic': 3, 'Monster': 4, 'Player': 5}
files = {}
for m in re.finditer(r'\[SoundIndex\.(\w+)\]\s*=\s*new DXSound\(SoundPath \+ @"([^"]+)", SoundType\.(\w+)\)(\s*\{[^}]*Loop\s*=\s*true[^}]*\})?', mgr_src):
    name, file, kind, loop = m.group(1), m.group(2), m.group(3), bool(m.group(4))
    if name in sound_index:
        files[sound_index[name]] = (name, file, channels[kind], loop)

# Monster sounds.
mon_cases = re.findall(r'((?:\s*case MonsterImage\.\w+:\s*)+)(.*?)break;', mon_src, re.S)
monster_sounds = {}
for labels, body in mon_cases:
    names = re.findall(r'MonsterImage\.(\w+)', labels)
    def pick(field):
        mm = re.search(field + r'\s*=\s*SoundIndex\.(\w+)', body)
        return sound_index.get(mm.group(1), 0) if mm else 0
    trio = (pick('AttackSound'), pick('StruckSound'), pick('DieSound'))
    if trio == (0, 0, 0):
        continue
    for n in names:
        if n in monster_image:
            monster_sounds[monster_image[n]] = trio


def switch_sounds(start_at, end_at):
    """Collect `case MagicType.X:` -> SoundIndex names inside a line range."""
    lines = map_src.split('\n')
    text = '\n'.join(lines[start_at - 1:end_at])
    out = {}
    parts = re.split(r'(case MagicType\.\w+:)', text)
    current = []
    for part in parts:
        m = re.match(r'case MagicType\.(\w+):', part)
        if m:
            current.append(m.group(1))
            continue
        if not current:
            continue
        if part.strip():
            sounds = re.findall(r'DXSoundManager\.Play\(SoundIndex\.(\w+)\)', part)
            sounds = [s for s in sounds if s in sound_index]
            for c in current:
                out.setdefault(c, [])
                for s in sounds:
                    if s not in out[c]:
                        out[c].append(s)
            current = []
    return out


lines = map_src.split('\n')
def find_line(pattern, start=1):
    for i in range(start - 1, len(lines)):
        if re.search(pattern, lines[i]):
            return i + 1
    raise SystemExit('not found: ' + pattern)


resolve_start = find_line(r'case MirAction\.Spell:')
# The resolve switch runs from the first `case MirAction.Spell:` up to the
# attack switch of SetAction; the cast switch is the second Spell case.
attack_start = find_line(r'case MirAction\.Attack:', resolve_start)
cast_start = find_line(r'case MirAction\.Spell:', attack_start)
cast_end = len(lines)
resolve_all = switch_sounds(resolve_start, attack_start - 1)
attack = switch_sounds(attack_start, cast_start - 1)
cast = switch_sounds(cast_start, cast_end)
# Travel sounds accompany the projectile; everything else in the resolve
# block plays at impact.
travel = {k: [s for s in v if s.endswith('Travel')] for k, v in resolve_all.items()}
resolve = {k: [s for s in v if not s.endswith('Travel')] for k, v in resolve_all.items()}

def emit_map(fn, doc, table, arity):
    print(f'/// {doc}')
    if arity == 1:
        print(f'pub fn {fn}(magic: u16) -> &\'static [u16] {{')
        print('    match magic {')
        for name, sounds in sorted(table.items(), key=lambda kv: magic_type.get(kv[0], 0)):
            if name not in magic_type or not sounds:
                continue
            ids = ', '.join(str(sound_index[s]) for s in sounds)
            print(f'        {magic_type[name]} => &[{ids}], // {name}: {", ".join(sounds)}')
        print('        _ => &[],')
        print('    }')
        print('}')
        print()


print('//! Generated by rust/tools/gen_sound_table.py from the C# client. Do not edit.')
print('//! Zircon `SoundIndex` values, their files and channels, and the sounds')
print('//! monsters and magics play.')
print()
print('/// Channels (Zircon `SoundType`): System 1, Music 2, Magic 3, Monster 4, Player 5.')
print('pub const CHANNELS: usize = 6;')
print()
print('/// `SoundIndex` -> (file name under Sound/, channel, loops).')
print('pub fn sound_file(index: u16) -> Option<(&\'static str, u8, bool)> {')
print('    Some(match index {')
for idx in sorted(files):
    name, file, kind, loop = files[idx]
    print(f'        {idx} => ("{file}", {kind}, {"true" if loop else "false"}), // {name}')
print('        _ => return None,')
print('    })')
print('}')
print()
print('/// `MonsterImage` -> (attack, struck, die) sound indices (0 = none).')
print('pub fn monster_sounds(image: u16) -> (u16, u16, u16) {')
print('    match image {')
for img in sorted(monster_sounds):
    a, s, d = monster_sounds[img]
    names = [k for k, v in monster_image.items() if v == img]
    print(f'        {img} => ({a}, {s}, {d}), // {names[0] if names else img}')
print('        _ => (0, 0, 0),')
print('    }')
print('}')
print()
emit_map('magic_cast', 'Sounds played when a magic is cast (`MirAction.Spell` start).', cast, 1)
emit_map('magic_travel', 'Sounds played while a magic projectile travels.', travel, 1)
emit_map('magic_end', 'Sounds played when a magic resolves (impact).', resolve, 1)
emit_map('attack_magic', 'Sounds played on a melee swing carrying a magic.', attack, 1)

named = ['FishingCast', 'FishingBob', 'FishingReel', 'MiningHit', 'MiningStruck',
         'ButtonA', 'ButtonC', 'TeleportOut', 'TeleportIn', 'ItemPotion', 'ItemWeapon', 'ItemArmour',
         'ItemRing', 'ItemBracelet', 'ItemNecklace', 'ItemHelmet', 'ItemShoes', 'ItemDefault',
         'GoldPickUp', 'GoldGained', 'DaggerSwing', 'WoodSwing', 'IronSwordSwing', 'ShortSwordSwing',
         'AxeSwing', 'WandSwing', 'FistSwing', 'GlaiveAttack', 'ClawAttack', 'GenericStruckPlayer',
         'GenericStruckMonster', 'Foot1', 'Foot2', 'Foot3', 'Foot4', 'MaleStruck', 'FemaleStruck',
         'MaleDie', 'FemaleDie', 'QuestTake', 'QuestComplete', 'LoginScene', 'SelectScene',
         'LightningStrikeEnd', 'FullBloom', 'WhiteLotus', 'RedLotus', 'SweetBrier', 'Karma',
         'FlashOfLightEnd', 'ParasiteExplode', 'FireStormEnd', 'SummonSkeletonEnd', 'SummonShinsuEnd',
         'HundredFist', 'GreaterIceBoltEnd', 'FireWallDuration', 'TempestDuration', 'PoisonousCloudStart',
         'DarkSoulPrison', 'StoneGolemAppear', 'ZumaKingAppear', 'ShinsuShow', 'ElementalSwordsEnd',
         'GreaterFireBallEnd', 'CursedDollEnd', 'SummonDeadEnd']
print('/// Indices the client triggers directly.')
print('#[allow(dead_code)]')
print('pub mod idx {')
for n in named:
    if n in sound_index:
        const = re.sub(r'(?<!^)(?=[A-Z])', '_', n).upper()
        print(f'    pub const {const}: u16 = {sound_index[n]};')
print('}')
sys.stderr.write(f'files {len(files)} monsters {len(monster_sounds)} cast {len(cast)} resolve {len(resolve)} attack {len(attack)}\n')

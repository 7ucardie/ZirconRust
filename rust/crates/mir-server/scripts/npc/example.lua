-- Example NPC page script (Zircon NpcScriptEngine API).
--
-- A page names this file in ScriptFile; its Script checks and actions name
-- the functions below in StringParameter1. Every function receives
-- (player, npc):
--   player.name, player.level, player.gold, player.has_item(name [, count])
--   npc.give_gold(n), npc.take_gold(n), npc.give_item(name [, count]),
--   npc.take_item(name [, count]), npc.message(text), npc.navigate(page)
-- on_open(player, npc) runs when the page opens; npc.navigate("Page
-- description") sends the player to another page and npc.navigate("")
-- closes the dialog.

function is_rich(player, npc)
    return player.gold >= 1000
end

function has_potion(player, npc)
    return player.has_item("Healing Potion", 1)
end

function reward(player, npc)
    npc.take_gold(1000)
    npc.give_item("Healing Potion", 2)
    npc.message("Enjoy the potions, " .. player.name .. ".")
end

function on_open(player, npc)
    if player.level >= 99 then
        npc.navigate("")
    end
end

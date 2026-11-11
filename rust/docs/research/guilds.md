# Research: guilds (from the C# server)

Extracted 2026-09-13 by direct reading of ServerLibrary/Models/
PlayerObject.Social.cs (`GuildCreate` 603, `GuildEditNotice` 703,
`GuildEditMember` 722, `GuildKickMember` 773, `GuildTax` 828,
`GuildIncreaseMember` 849, `GuildIncreaseStorage` 885, `GuildInviteMember`
919, `GuildWar` 973, `GuildJoin` 1360, `GuildLeave` 1431), DBModels/
GuildInfo.cs + GuildMemberInfo.cs, LibraryCore/Globals.cs 44-45, 79-83,
124-127, 907-952, Enum.cs `GuildPermission` 1808, Network packets.

- Membership is per account (`Account.GuildMember`); `GuildMemberInfo`
  has Rank (string), Permission (flags: Leader = -1, EditNotice 1,
  AddMember 2, RemoveMember 4, Storage 8, FundsRepair 16, FundsMerchant
  32, FundsMarket 64, StartWar 128), JoinDate, contributions.
- `GuildInfo`: GuildName, MemberLimit, StorageSize, GuildFunds,
  GuildLevel, GuildNotice (<= 4000), GuildTax (0-100%), Default
  Rank/Permission ("New Member"), StarterGuild, Conquest/Castle, Colour,
  Flag.
- Create (`C.GuildCreate { Name, UseGold, Members, Storage }`): not in a
  guild; name matches `^[A-Za-z0-9]{2,15}$` and is unique
  (case-insensitive); cost = Members * 1,000,000 + Storage * 350,000 (+
  7,500,000 with UseGold, otherwise an UmaKingHorn item); Members <= 100,
  Storage <= 500. Creator gets rank "Guild Leader", permission Leader.
- Invite (`GuildInviteMember { Name }`): needs AddMember; target online,
  not in a guild, not already invited, `AllowGuild`, room under
  MemberLimit; `S.GuildInvite { Name, GuildName }`. `GuildJoin`
  re-checks the inviter's permission and room, honours
  `Account.GuildTime` (rejoin delay), adds with the default rank and
  permission, broadcasts `S.GuildChanged { ObjectID, GuildName, GuildRank }`
  and sends `S.GuildUpdate` to members.
- Edit member (`GuildEditMember { Index, Rank, Permission }`): Leader
  only; rank <= 15 chars; Index 0 edits the defaults; one cannot change
  one's own permission. Kick: Leader only, not self. Leave: a leader may
  not leave a guild with other members unless another leader exists.
- Tax (`GuildTax { Tax 0-100 }`): Leader; `CalculateGuildTax` takes the
  cut from gold drops and sales for the funds. Increase member limit:
  Leader, limit < 100, funds >= 1,000,000. Increase storage: < 500,
  350,000.
- Chat `!~` reaches online members (`MessageType.Guild`).
- Client view: `S.GuildInfo { ClientGuildInfo }` with members (Index,
  Name, Rank, contributions, Online, Permission, ObjectID),
  `S.GuildNoticeChanged`, `S.GuildUpdate`, `S.GuildKick { Index }`,
  `S.GuildMemberOnline/Offline`, `S.GuildFundsChanged`.
- Not in the prototype: guild storage, wars (`GuildWar`, 200,000), castles
  and conquests, colours/flags, contributions and daily growth, guild
  buffs, rejoin delay, AllowGuild toggle, StarterGuild. Membership here
  is per character and kept in `guilds.json` under the data dir.

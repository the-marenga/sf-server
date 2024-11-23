DROP INDEX GuildHoF;
DROP INDEX HoF;

DROP TABLE Guild;
DROP TABLE GuildUpgrade;
DROP TABLE Item;
DROP TABLE LoginData;
DROP TABLE Quest;
DROP TABLE Activity;
DROP TABLE Equipment;
DROP TABLE Portrait;
DROP TABLE ChatMessage;
DROP TABLE Character;
DROP TABLE GuildMember;
DROP TABLE Server;
DROP Table Bag;
DROP TABLE Attributes;
DROP TABLE Tavern;

-- This can be something like w1, w2, etc. You could and probably should have
-- these be actual distinct instances of the server. Since that is way more
-- annoying to setup though, this will be all in one server. That also allows
-- creating private world at a later point in time
CREATE TABLE world (
  world_id INTEGER PRIMARY KEY autoincrement NOT NULL,
  ident TEXT NOT NULL UNIQUE
);

-- Add a default world
INSERT INTO world (ident) VALUES ('');

CREATE TABLE guild (
  id INTEGER PRIMARY KEY autoincrement NOT NULL,
  world_id INT NOT NULL DEFAULT 1 REFERENCES world (id) ON DELETE cascade,

  name TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  emblem TEXT NOT NULL,

  raid INT NOT NULL DEFAULT 0,
  honor INT NOT NULL DEFAULT 200,
  created INT NOT NULL,

  demon_portal_act INT NOT NULL DEFAULT 1,
  demon_portal_health INT NOT NULL DEFAULT 1,

  catapult INT NOT NULL DEFAULT 0 CHECK (catapult < 4),
  attacking INT REFERENCES guild (id),

  pet_id INT,
  hydra_heads INT,
  hydra_current_life INT NOT NULL,
  UNIQUE (world_id, name)
);

CREATE index guild_hof ON guild (world_id, honor DESC, id ASC);

-- Everything related to the Membership of a character to a guild
CREATE TABLE guild_member (
  pid INTEGER PRIMARY KEY NOT NULL,
  guild_id INT NOT NULL REFERENCES guild (id) ON DELETE CASCADE,
  -- 1 => Leader
  -- 2 => Member
  -- 3 => Leader
  rank INT NOT NULL CHECK (rank < 4),
  joined INT NOT NULL,
  last_active INT NOT NULL,

  is_defending BOOL NOT NULL DEFAULT FALSE,
  is_attacking BOOL NOT NULL DEFAULT FALSE,

  hydra_fought BOOL NOT NULL DEFAULT FALSE,
  portal_fought BOOL NOT NULL DEFAULT FALSE,

  FOREIGN KEY (pid) REFERENCES character (pid)
);

-- The guild upgrades a character has for himself
CREATE TABLE guild_upgrade (
  pid INTEGER PRIMARY KEY NOT NULL,
  treasure INT NOT NULL DEFAULT 0,
  instructor INT NOT NULL DEFAULT 0,
  petlvl INT NOT NULL DEFAULT 0
);

CREATE TABLE item (
  id INTEGER PRIMARY KEY autoincrement NOT NULL,
  enchantment INT NOT NULL DEFAULT 0,
  item_type INT NOT NULL,
  effect1 INT NOT NULL DEFAULT 0,
  effect2 INT NOT NULL DEFAULT 0,
  ident INT NOT NULL DEFAULT 0,
  count INT NOT NULL DEFAULT 0,
  expires INT,
  gem_type INT NOT NULL DEFAULT 0,
  gem_power INT NOT NULL DEFAULT 0,
  class INT NOT NULL DEFAULT 0,
  atr_typ1 INT NOT NULL DEFAULT 0,
  atr_val1 INT NOT NULL DEFAULT 0,
  atr_typ2 INT NOT NULL DEFAULT 0,
  atr_val2 INT NOT NULL DEFAULT 0,
  atr_typ3 INT NOT NULL DEFAULT 0,
  atr_val3 INT NOT NULL DEFAULT 0,
  model_id INT NOT NULL,
  silver INT NOT NULL,
  mushrooms INT NOT NULL
);

CREATE TABLE bag (
  pid INTEGER PRIMARY KEY NOT NULL,
  pos1 INT REFERENCES item (id) ON DELETE SET NULL,
  pos2 INT REFERENCES item (id) ON DELETE SET NULL,
  pos3 INT REFERENCES item (id) ON DELETE SET NULL,
  pos4 INT REFERENCES item (id) ON DELETE SET NULL,
  pos5 INT REFERENCES item (id) ON DELETE SET NULL
);

CREATE TABLE attributes (
  id INTEGER PRIMARY KEY autoincrement NOT NULL,
  strength INT NOT NULL DEFAULT 0,
  dexterity INT NOT NULL DEFAULT 0,
  intelligence INT NOT NULL DEFAULT 0,
  stamina INT NOT NULL DEFAULT 0,
  luck INT NOT NULL DEFAULT 0
);

CREATE TABLE session (
  id INTEGER PRIMARY KEY autoincrement NOT NULL,
  pid INT NOT NULL REFERENCES character (pid) ON DELETE CASCADE,
  session_id TEXT NOT NULL,
  crypto_id TEXT UNIQUE NOT NULL,
  login_count INT NOT NULL DEFAULT 0
);

CREATE TABLE quest (
  id INTEGER PRIMARY KEY autoincrement NOT NULL,
  flavour1 INT NOT NULL DEFAULT 1,
  flavour2 INT NOT NULL DEFAULT 1,
  monster INT NOT NULL,
  location INT NOT NULL,
  length INT NOT NULL,
  xp INT NOT NULL,
  silver INT NOT NULL,
  mushrooms INT NOT NULL DEFAULT 0,
  item INT REFERENCES item (id) ON DELETE SET NULL
);

CREATE TABLE activity (
  pid INTEGER PRIMARY KEY NOT NULL,
  typ INT NOT NULL DEFAULT 0,
  sub_type INT NOT NULL DEFAULT 0,
  started INT NOT NULL DEFAULT 0,
  busy_until INT NOT NULL DEFAULT 0
);

CREATE TABLE tavern (
  pid INTEGER PRIMARY KEY autoincrement NOT NULL,
  quest1 INT NOT NULL REFERENCES quest (id),
  quest2 INT NOT NULL REFERENCES quest (id),
  quest3 INT NOT NULL REFERENCES quest (id),
  tfa INT NOT NULL DEFAULT 6000,
  beer_drunk INT NOT NULL DEFAULT 0,
  quicksand INT NOT NULL DEFAULT 60,
  dice_games_remaining INT NOT NULL DEFAULT 10,
  dice_game_next_free INT NOT NULL DEFAULT 0
);

-- This is only character equipment
CREATE TABLE equipment (
  pid INTEGER PRIMARY KEY NOT NULL,
  hat INT REFERENCES item (id) ON DELETE SET NULL,
  breastplate INT REFERENCES item (id) ON DELETE SET NULL,
  gloves INT REFERENCES item (id) ON DELETE SET NULL,
  footwear INT REFERENCES item (id) ON DELETE SET NULL,
  amulet INT REFERENCES item (id) ON DELETE SET NULL,
  belt INT REFERENCES item (id) ON DELETE SET NULL,
  ring INT REFERENCES item (id) ON DELETE SET NULL,
  talisman INT REFERENCES item (id) ON DELETE SET NULL,
  weapon INT REFERENCES item (id) ON DELETE SET NULL,
  shield INT REFERENCES item (id) ON DELETE SET NULL
);

CREATE TABLE portrait (
  pid INTEGER PRIMARY KEY NOT NULL,
  mouth INT NOT NULL DEFAULT 1,
  hair INT NOT NULL DEFAULT 1,
  brows INT NOT NULL DEFAULT 1,
  eyes INT NOT NULL DEFAULT 1,
  beards INT NOT NULL DEFAULT 1,
  nose INT NOT NULL DEFAULT 1,
  ears INT NOT NULL DEFAULT 1,
  extra INT NOT NULL DEFAULT 1,
  horns INT NOT NULL DEFAULT 1,
  influencer INT NOT NULL DEFAULT 0
);

CREATE TABLE character (
  pid INTEGER PRIMARY KEY NOT NULL,
  world_id INT NOT NULL REFERENCES world (world_id) ON DELETE cascade,

  crypto_key TEXT NOT NULL,
  mail TEXT UNIQUE,
  pw_hash TEXT NOT NULL,

  name TEXT NOT NULL,
  class INT NOT NULL,
  race INT NOT NULL,
  gender INT NOT NULL,
  level INT NOT NULL DEFAULT 1,
  experience INT NOT NULL DEFAULT 0,
  honor INT NOT NULL DEFAULT 300,
  silver INT NOT NULL DEFAULT 100,
  mushrooms INT NOT NULL DEFAULT 30,
  description TEXT NOT NULL DEFAULT '',
  mount INT NOT NULL DEFAULT 0,
  mount_end INT NOT NULL DEFAULT 0,
  tutorial_status INT NOT NULL DEFAULT 0,

  attributes INT NOT NULL REFERENCES attributes (id),
  attributes_bought INT NOT NULL REFERENCES attributes (id),

  FOREIGN KEY (pid) REFERENCES guild_upgrade (pid),
  FOREIGN KEY (pid) REFERENCES equipment (pid),
  FOREIGN KEY (pid) REFERENCES activity (pid),
  FOREIGN KEY (pid) REFERENCES portrait (pid),
  FOREIGN KEY (pid) REFERENCES bag (pid),
  FOREIGN KEY (pid) REFERENCES tavern (pid),

  UNIQUE (name, world_id)
);

CREATE index character_hof ON character(world_id, honor DESC, pid ASC);

CREATE TABLE chat_message (
  id INTEGER PRIMARY KEY autoincrement NOT NULL,
  sender INT REFERENCES character(pid) ON DELETE CASCADE,
  time INT NOT NULL,
  guild INT REFERENCES guild (id),
  whisper INT REFERENCES character(pid),
  message TEXT NOT NULL,
  is_global BOOL NOT NULL
);

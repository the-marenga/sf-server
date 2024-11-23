CREATE TABLE Server (
    ID INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
    ident TEXT NOT NULL
);

-- Add default derver
INSERT INTO SERVER (ident) VALUES ('');

CREATE TABLE Guild (
	ID INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
	server INT NOT NULL DEFAULT 1 REFERENCES Server (ID) ON DELETE CASCADE,
	Name TEXT NOT NULL,
	Description TEXT,
	Emblem TEXT,
	Raid INT NOT NULL DEFAULT 0,
	Honor INT NOT NULL DEFAULT 200,
	Created BIGINT NOT NULL DEFAULT CURRENT_BIGINT,
	MemberLimit INT NOT NULL DEFAULT 30,
	DemonPortalAct INT NOT NULL DEFAULT 1,
	DemonPortalHealth INT NOT NULL DEFAULT 1,
	Catapult INT NOT NULL DEFAULT 0 CHECK (Catapult < 4),
	PetId INT,
	HydraCurrentLife INT NOT NULL,
	Attacking INT REFERENCES Guild (ID),
	UNIQUE(server, name)
);

CREATE INDEX GuildHoF ON Guild (server, Honor DESC, ID ASC);


-- Everything related to the Membership of a player to a guild
CREATE TABLE GuildMember (
	ID INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
	Guild INT NOT NULL REFERENCES Guild (ID) ON DELETE CASCADE,
	-- 3 == Leader
	Rank INT NOT NULL,
	Joined BIGINT DEFAULT CURRENT_BIGINT,
	LastActive BIGINT DEFAULT CURRENT_BIGINT,
	IsDefending BOOL NOT NULL DEFAULT FALSE,
	IsAttacking BOOL NOT NULL DEFAULT FALSE,
	-- The amount of times we fought against the hydra today
	HydraFoughtCount INT NOT NULL DEFAULT 0,
	-- Last time we fought with pet against the hydra
	HydraLastFought BIGINT,
	-- Last time we fought in the portal
	PortalLastFought BIGINT
);

-- The guild upgrades a player has for himself
CREATE TABLE GuildUpgrade (
    ID INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
	Treasure INT NOT NULL DEFAULT 0,
	Instructor INT NOT NULL DEFAULT 0,
	PetLvl INT NOT NULL DEFAULT 0
);


CREATE TABLE Item (
    ID INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
	Enchantment INT NOT NULL DEFAULT 0,
	ItemTyp INT NOT NULL,

	Effect1 INT NOT NULL DEFAULT 0,
	Effect2 INT NOT NULL DEFAULT 0,

	Ident INT NOT NULL DEFAULT 0,
	Count INT NOT NULL DEFAULT 0,
	Expires BIGINT,

	GemType INT NOT NULL DEFAULT 0,
	GemPower INT NOT NULL DEFAULT 0,

	Class INT NOT NULL DEFAULT 0,

	AtrTyp1 INT NOT NULL DEFAULT 0,
	AtrVal1 INT NOT NULL DEFAULT 0,
	AtrTyp2 INT NOT NULL DEFAULT 0,
	AtrVal2 INT NOT NULL DEFAULT 0,
	AtrTyp3 INT NOT NULL DEFAULT 0,
	AtrVal3 INT NOT NULL DEFAULT 0,

	ModelID INT NOT NULL,

	Silver INT NOT NULL,
	Mushrooms INT NOT NULL
);

CREATE Table Bag (
	ID INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
	pos1 INT REFERENCES ITEM(ID) ON DELETE CASCADE,
	pos2 INT REFERENCES ITEM(ID) ON DELETE CASCADE,
	pos3 INT REFERENCES ITEM(ID) ON DELETE CASCADE,
	pos4 INT REFERENCES ITEM(ID) ON DELETE CASCADE,
	pos5 INT REFERENCES ITEM(ID) ON DELETE CASCADE
);

CREATE TABLE Attributes (
    ID INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
	Strength INT NOT NULL DEFAULT 0,
	Dexterity INT NOT NULL DEFAULT 0,
	Intelligence INT NOT NULL DEFAULT 0,
	Stamina INT NOT NULL DEFAULT 0,
	Luck INT NOT NULL DEFAULT 0
);

CREATE TABLE LoginData (
	ID INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
	Mail TEXT UNIQUE NOT NULL,
	PWHash TEXT NOT NULL,
	SessionID Text NOT NULL,
	CryptoID Text UNIQUE NOT NULL,
	CryptoKey Text NOT NULL,
	LoginCount INT NOT NULL DEFAULT 0
);

CREATE TABLE Quest (
    ID INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
	Flavour1 INT NOT NULL DEFAULT 1,
	Flavour2 INT NOT NULL DEFAULT 1,
	Monster INT NOT NULL,
	Location INT NOT NULL,
	Length INT NOT NULL,
	XP INT NOT NULL,
	Silver INT NOT NULL,
	Mushrooms INT NOT NULL DEFAULT 0,
	Item Int REFERENCES Item(ID)
);

CREATE TABLE Activity (
    ID INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
	Typ INT NOT NULL DEFAULT 0,
	SubTyp INT NOT NULL DEFAULT 0,
	Started BIGINT NOT NULL DEFAULT 0,
	BusyUntil BIGINT NOT NULL DEFAULT 0
);

CREATE TABLE Tavern (
    ID INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,

	Quest1 INT NOT NULL REFERENCES Quest(ID),
	Quest2 INT NOT NULL REFERENCES Quest(ID),
	Quest3 INT NOT NULL REFERENCES Quest(ID),

	TFA INT NOT NULL DEFAULT 6000,
	BeerDrunk INT NOT NULL DEFAULT 0,
	QuickSand INT NOT NULL DEFAULT 60,

	DiceGamesRemaining INT NOT NULL DEFAULT 10,
	DiceGameNextFree BIGINT NOT NULL DEFAULT 0
);

CREATE TABLE Equipment (
	ID INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
	Hat INT REFERENCES ITEM (id),
	BreastPlate INT REFERENCES ITEM (id),
	Gloves INT REFERENCES ITEM (id),
	FootWear INT REFERENCES ITEM (id),
	Amulet INT REFERENCES ITEM (id),
	Belt INT REFERENCES ITEM (id),
	Ring INT REFERENCES ITEM (id),
	Talisman INT REFERENCES ITEM (id),
	Weapon INT REFERENCES ITEM (id),
	Shield INT REFERENCES ITEM (id)
);

CREATE TABLE Portrait (
	ID INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
	Mouth INT NOT NULL DEFAULT 1,
	Hair INT NOT NULL DEFAULT 1,
	Brows INT NOT NULL DEFAULT 1,
	Eyes INT NOT NULL DEFAULT 1,
	Beards  INT NOT NULL DEFAULT 1,
	Nose  INT NOT NULL DEFAULT 1,
	Ears  INT NOT NULL DEFAULT 1,
	Extra  INT NOT NULL DEFAULT 1,
	Horns INT NOT NULL DEFAULT 1,
	Influencer INT NOT NULL DEFAULT 0
);

CREATE TABLE Character (
	ID INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
	server INT NOT NULL DEFAULT 1 REFERENCES Server (ID) ON DELETE CASCADE,
	Name TEXT NOT NULL,
	Class INT NOT NULL,
	Race INT NOT NULL,
	Gender INT NOT NULL,
	Level INT NOT NULL DEFAULT 1,
	Experience BIGINT NOT NULL DEFAULT 0,
	Honor INT NOT NULL DEFAULT 300,
	Silver BIGINT NOT NULL DEFAULT 100,
	Mushrooms INT NOT NULL DEFAULT 30,
	Bag INT NOT NULL REFERENCES Bag (ID) ON DELETE CASCADE,
	Attributes INT NOT NULL REFERENCES Attributes (ID) ON DELETE CASCADE,
	AttributesBought INT NOT NULL REFERENCES Attributes (ID) ON DELETE CASCADE,
	LoginData INT NOT NULL REFERENCES LoginData (ID) ON DELETE CASCADE,
	Tavern INT NOT NULL REFERENCES Tavern (ID) ON DELETE CASCADE,
	Portrait INT NOT NULL REFERENCES Portrait(ID),
	Activity INT NOT NULL REFERENCES Activity(ID),
	Equipment INT NOT NULL REFERENCES Equipment(id),
	Description TEXT NOT NULL DEFAULT '',
	Mount INT NOT NULL DEFAULT 0,
	MountEnd BIGINT NOT NULL DEFAULT 0,
	TutorialStatus INT NOT NULL DEFAULT 0,
	Guild INT REFERENCES GuildMember (ID) ON DELETE CASCADE,
	UNIQUE(name, server)
);

-- CREATE INDEX HoF ON Character (Honor DESC, ID ASC) INCLUDE (NAME, level, class);
CREATE INDEX HoF ON Character (server, Honor DESC, ID ASC);

CREATE TABLE ChatMessage (
	ID INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
	Sender INT REFERENCES CHARACTER(ID),
	Send BIGINT NOT NULL DEFAULT CURRENT_BIGINT,
	Guild INT REFERENCES Guild(ID),
	Whisper INT REFERENCES Character(ID),
	Messafe TEXT NOT NULL,
	IsGlobal BOOLEAN NOT NULL
);

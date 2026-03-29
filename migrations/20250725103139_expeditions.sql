CREATE TABLE Expedition (
    id INTEGER PRIMARY KEY autoincrement NOT NULL,
    pid INTEGER NOT NULL REFERENCES character (pid) ON DELETE cascade,
    target INT NOT NULL,
    alu_sec INT NOT NULL,
    location_1 INT NOT NULL,
    location_2 INT NOT NULL
);

CREATE INDEX ExpeditionPlayer ON Expedition(pid);

CREATE TABLE ActiveExpedition (
    pid INTEGER PRIMARY KEY NOT NULL REFERENCES character (pid) ON DELETE cascade,
    -- ExpeditionThing
    target INT NOT NULL,

    item1 INT DEFAULT NULL,
    item2 INT DEFAULT NULL,
    item3 INT DEFAULT NULL,
    item4 INT DEFAULT NULL,

    target_current INT NOT NULL DEFAULT 0,
    target_amount INT NOT NULL DEFAULT 0,

    current_floor INT NOT NULL DEFAULT 1,
    heroism INT NOT NULL DEFAULT 0,

    floor_stage INT NOT NULL DEFAULT 0,

    encounter1 INT NOT NULL DEFAULT 0,
    encounter2 INT NOT NULL DEFAULT 0,
    encounter3 INT NOT NULL DEFAULT 0,
    encounter4 INT NOT NULL DEFAULT 0,

    boss_id INT NOT NULL,

    reward1_type INT NOT NULL DEFAULT 0,
    reward1_amount INT NOT NULL DEFAULT 0,
    reward2_type INT NOT NULL DEFAULT 0,
    reward2_amount INT NOT NULL DEFAULT 0,
    reward3_type INT NOT NULL DEFAULT 0,
    reward3_amount INT NOT NULL DEFAULT 0,
    reward4_type INT NOT NULL DEFAULT 0,
    reward4_amount INT NOT NULL DEFAULT 0
);

INSERT INTO Expedition (target, alu_sec, location_1, location_2, pid)
SELECT 33, 1500, 18, 12, pid FROM Character c;

INSERT INTO Expedition (target, alu_sec, location_1, location_2, pid)
SELECT 93, 1500, 18, 12, pid FROM Character c;

CREATE TABLE UserSetting (
    pid INTEGER PRIMARY KEY autoincrement NOT NULL REFERENCES character(pid),
    lang TEXT NOT NULL,
    quest_pref TEXT NOT NULL DEFAULT 'a'
);

INSERT INTO UserSetting (pid, lang, quest_pref)
SELECT pid, 'en', 'a' FROM Character c;

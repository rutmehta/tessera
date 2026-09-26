BEGIN IMMEDIATE;
-- IF NOT EXISTS permits historical face-migration replay in repair/tests.
CREATE TABLE IF NOT EXISTS person (
    id TEXT PRIMARY KEY NOT NULL CHECK(length(trim(id)) > 0),
    name TEXT,
    medoid TEXT
);
CREATE INDEX IF NOT EXISTS person_name_idx ON person(name, id);
CREATE TABLE IF NOT EXISTS face_person (
    image_id TEXT NOT NULL,
    ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
    person_id TEXT NOT NULL REFERENCES person(id) ON DELETE CASCADE,
    confirmed INTEGER NOT NULL DEFAULT 0 CHECK(confirmed IN (0,1)),
    PRIMARY KEY(image_id, ordinal),
    FOREIGN KEY(image_id, ordinal) REFERENCES face(image_id, id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS face_person_search_idx ON face_person(person_id, image_id, confirmed);
CREATE INDEX IF NOT EXISTS face_person_confirmed_idx ON face_person(person_id, confirmed, image_id);
INSERT INTO migration(version) VALUES(8);
COMMIT;

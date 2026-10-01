-- Explicit originals may have a folder without registering a library root.
-- Rebuild only the parent table, preserving IDs and all dependent file rows.
PRAGMA foreign_keys=OFF;
BEGIN IMMEDIATE;
CREATE TABLE folder_new(
    id INTEGER PRIMARY KEY,
    root_id INTEGER REFERENCES root(id),
    path TEXT NOT NULL UNIQUE
);
INSERT INTO folder_new(id,root_id,path) SELECT id,root_id,path FROM folder;
DROP TABLE folder;
ALTER TABLE folder_new RENAME TO folder;
INSERT INTO migration(version) VALUES(10);
COMMIT;
PRAGMA foreign_keys=ON;

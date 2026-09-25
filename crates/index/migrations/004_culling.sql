BEGIN IMMEDIATE;
CREATE TABLE score (
    image_id TEXT NOT NULL REFERENCES image(id) ON DELETE CASCADE,
    signal TEXT NOT NULL,
    value REAL NOT NULL,
    model TEXT NOT NULL,
    PRIMARY KEY(image_id, signal)
);
CREATE TABLE export_log (
    id INTEGER PRIMARY KEY,
    image_id TEXT NOT NULL REFERENCES image(id) ON DELETE CASCADE,
    destination TEXT NOT NULL,
    published INTEGER NOT NULL DEFAULT 0 CHECK(published IN (0, 1)),
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX export_log_image_idx ON export_log(image_id);
INSERT INTO migration(version) VALUES(4);
COMMIT;

BEGIN IMMEDIATE;
CREATE TABLE face (
    image_id TEXT NOT NULL REFERENCES image(id) ON DELETE CASCADE,
    id INTEGER NOT NULL CHECK(id >= 0),
    bbox TEXT NOT NULL,
    landmarks5 TEXT NOT NULL,
    confidence REAL NOT NULL CHECK(confidence BETWEEN 0 AND 1),
    embedding TEXT,
    model TEXT NOT NULL,
    PRIMARY KEY(image_id, id)
);
INSERT INTO migration(version) VALUES(5);
COMMIT;

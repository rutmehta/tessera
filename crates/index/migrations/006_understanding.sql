BEGIN IMMEDIATE;
CREATE TABLE accepted_keyword(
    image_id TEXT NOT NULL REFERENCES image(id) ON DELETE CASCADE,
    keyword_id INTEGER NOT NULL REFERENCES keyword(id) ON DELETE CASCADE,
    PRIMARY KEY(image_id,keyword_id)
);
CREATE TABLE understanding(
    image_id TEXT PRIMARY KEY REFERENCES image(id) ON DELETE CASCADE,
    model_version TEXT NOT NULL,
    keywords TEXT NOT NULL,
    caption TEXT NOT NULL,
    alt_text TEXT NOT NULL,
    ocr TEXT NOT NULL,
    ocr_text TEXT NOT NULL
);
CREATE TRIGGER image_fts_delete AFTER DELETE ON image BEGIN
    DELETE FROM fts WHERE rowid=old.rowid;
END;
INSERT INTO migration(version) VALUES(6);
COMMIT;

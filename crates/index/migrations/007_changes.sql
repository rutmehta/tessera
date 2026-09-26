-- Change feed (M2-28): every connection's writes to catalog rows append here in the
-- same transaction, so hosts can apply them incrementally instead of reloading.
-- kind: 1 added, 2 removed, 3 updated. fields (updated only) is a bit set:
-- 1 file/path, 2 capture time, 4 metadata (camera, lens, caption, GPS, orientation),
-- 8 selection, 16 recipe, 32 keywords, 64 scores.
BEGIN IMMEDIATE;
CREATE TABLE IF NOT EXISTS change_log(
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    image_id TEXT NOT NULL,
    kind INTEGER NOT NULL,
    fields INTEGER NOT NULL DEFAULT 0
);
-- Highest sequence dropped by trimming; pulls older than it must reload.
CREATE TABLE IF NOT EXISTS change_meta(key TEXT PRIMARY KEY, value INTEGER NOT NULL);
INSERT OR IGNORE INTO change_meta VALUES('trimmed', 0);
CREATE TRIGGER IF NOT EXISTS change_image_insert AFTER INSERT ON image BEGIN
    INSERT INTO change_log(image_id,kind) VALUES(new.id,1);
END;
CREATE TRIGGER IF NOT EXISTS change_image_delete AFTER DELETE ON image BEGIN
    INSERT INTO change_log(image_id,kind) VALUES(old.id,2);
END;
CREATE TRIGGER IF NOT EXISTS change_image_update AFTER UPDATE ON image
WHEN old.file_id IS NOT new.file_id OR old.capture_time IS NOT new.capture_time
    OR old.camera IS NOT new.camera OR old.lens IS NOT new.lens OR old.caption IS NOT new.caption
    OR old.latitude IS NOT new.latitude OR old.longitude IS NOT new.longitude
BEGIN
    INSERT INTO change_log(image_id,kind,fields) VALUES(new.id,3,
        (CASE WHEN old.file_id IS NOT new.file_id THEN 1 ELSE 0 END)
        | (CASE WHEN old.capture_time IS NOT new.capture_time THEN 2 ELSE 0 END)
        | (CASE WHEN old.camera IS NOT new.camera OR old.lens IS NOT new.lens OR old.caption IS NOT new.caption
                OR old.latitude IS NOT new.latitude OR old.longitude IS NOT new.longitude THEN 4 ELSE 0 END));
END;
CREATE TRIGGER IF NOT EXISTS change_file_update AFTER UPDATE ON file
WHEN old.path IS NOT new.path OR old.size IS NOT new.size OR old.mtime IS NOT new.mtime
BEGIN
    INSERT INTO change_log(image_id,kind,fields) SELECT id,3,1 FROM image WHERE file_id=new.id;
END;
CREATE TRIGGER IF NOT EXISTS change_selection_insert AFTER INSERT ON selection BEGIN
    INSERT INTO change_log(image_id,kind,fields) VALUES(new.image_id,3,8);
END;
CREATE TRIGGER IF NOT EXISTS change_selection_update AFTER UPDATE ON selection
WHEN old.decision IS NOT new.decision OR old.grade IS NOT new.grade OR old.mark IS NOT new.mark
BEGIN
    INSERT INTO change_log(image_id,kind,fields) VALUES(new.image_id,3,8);
END;
CREATE TRIGGER IF NOT EXISTS change_recipe_insert AFTER INSERT ON recipe_hash BEGIN
    INSERT INTO change_log(image_id,kind,fields) VALUES(new.image_id,3,16);
END;
CREATE TRIGGER IF NOT EXISTS change_recipe_update AFTER UPDATE ON recipe_hash
WHEN old.hash IS NOT new.hash
BEGIN
    INSERT INTO change_log(image_id,kind,fields) VALUES(new.image_id,3,16);
END;
CREATE TRIGGER IF NOT EXISTS change_recipe_delete AFTER DELETE ON recipe_hash BEGIN
    INSERT INTO change_log(image_id,kind,fields) VALUES(old.image_id,3,16);
END;
CREATE TRIGGER IF NOT EXISTS change_orientation_insert AFTER INSERT ON metadata
WHEN new.key='orientation'
BEGIN
    INSERT INTO change_log(image_id,kind,fields) VALUES(new.image_id,3,4);
END;
CREATE TRIGGER IF NOT EXISTS change_orientation_delete AFTER DELETE ON metadata
WHEN old.key='orientation'
BEGIN
    INSERT INTO change_log(image_id,kind,fields) VALUES(old.image_id,3,4);
END;
CREATE TRIGGER IF NOT EXISTS change_keyword_insert AFTER INSERT ON image_keyword BEGIN
    INSERT INTO change_log(image_id,kind,fields) VALUES(new.image_id,3,32);
END;
CREATE TRIGGER IF NOT EXISTS change_keyword_delete AFTER DELETE ON image_keyword BEGIN
    INSERT INTO change_log(image_id,kind,fields) VALUES(old.image_id,3,32);
END;
CREATE TRIGGER IF NOT EXISTS change_score_insert AFTER INSERT ON score BEGIN
    INSERT INTO change_log(image_id,kind,fields) VALUES(new.image_id,3,64);
END;
CREATE TRIGGER IF NOT EXISTS change_score_update AFTER UPDATE ON score
WHEN old.value IS NOT new.value
BEGIN
    INSERT INTO change_log(image_id,kind,fields) VALUES(new.image_id,3,64);
END;
CREATE TRIGGER IF NOT EXISTS change_score_delete AFTER DELETE ON score BEGIN
    INSERT INTO change_log(image_id,kind,fields) VALUES(old.image_id,3,64);
END;
INSERT OR IGNORE INTO migration(version) VALUES(7);
COMMIT;

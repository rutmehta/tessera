-- Drive-resident state (CS-1, docs/05 §2.3). Every record the drive carries has a
-- last-writer-wins stamp: `modified` (Unix ms) and `machine` (catalog_meta
-- 'machine'). Local writes are stamped by triggers and marked dirty for the
-- drive writer; imports from the drive set catalog_meta 'importing' so the
-- triggers leave the imported stamps alone. The person/face_person stamp
-- columns are added by `Core::open` (ALTER TABLE cannot be replayed).
BEGIN IMMEDIATE;
-- Per-image stamp of the analysis record (scores, faces, understanding, assignments).
CREATE TABLE IF NOT EXISTS drive_stamp(
    image_id TEXT PRIMARY KEY,
    modified INTEGER NOT NULL,
    machine TEXT NOT NULL,
    dirty INTEGER NOT NULL DEFAULT 1 CHECK(dirty IN (0,1))
);
CREATE INDEX IF NOT EXISTS drive_stamp_dirty_idx ON drive_stamp(dirty);
INSERT OR IGNORE INTO catalog_meta(key,value) VALUES('machine','');
-- Persons/assignments revision, and the revision the drive writer last wrote.
INSERT OR IGNORE INTO catalog_meta(key,value) VALUES('people_rev','0');
INSERT OR IGNORE INTO catalog_meta(key,value) VALUES('people_flushed','0');

-- Person records.
CREATE TRIGGER IF NOT EXISTS drive_person_insert AFTER INSERT ON person
WHEN NOT EXISTS(SELECT 1 FROM catalog_meta WHERE key='importing') BEGIN
    UPDATE person SET modified=CAST((julianday('now')-2440587.5)*86400000 AS INTEGER),
        machine=(SELECT value FROM catalog_meta WHERE key='machine') WHERE id=new.id;
    UPDATE catalog_meta SET value=CAST(value AS INTEGER)+1 WHERE key='people_rev';
END;
CREATE TRIGGER IF NOT EXISTS drive_person_update AFTER UPDATE OF name,medoid ON person
WHEN NOT EXISTS(SELECT 1 FROM catalog_meta WHERE key='importing') BEGIN
    UPDATE person SET modified=CAST((julianday('now')-2440587.5)*86400000 AS INTEGER),
        machine=(SELECT value FROM catalog_meta WHERE key='machine') WHERE id=new.id;
    UPDATE catalog_meta SET value=CAST(value AS INTEGER)+1 WHERE key='people_rev';
END;
CREATE TRIGGER IF NOT EXISTS drive_person_delete AFTER DELETE ON person
WHEN NOT EXISTS(SELECT 1 FROM catalog_meta WHERE key='importing') BEGIN
    UPDATE catalog_meta SET value=CAST(value AS INTEGER)+1 WHERE key='people_rev';
END;

-- Face assignments: stamp the row and the image's analysis record.
CREATE TRIGGER IF NOT EXISTS drive_face_person_insert AFTER INSERT ON face_person
WHEN NOT EXISTS(SELECT 1 FROM catalog_meta WHERE key='importing') BEGIN
    UPDATE face_person SET modified=CAST((julianday('now')-2440587.5)*86400000 AS INTEGER),
        machine=(SELECT value FROM catalog_meta WHERE key='machine')
        WHERE image_id=new.image_id AND ordinal=new.ordinal;
    INSERT INTO drive_stamp(image_id,modified,machine,dirty) VALUES(new.image_id,
        CAST((julianday('now')-2440587.5)*86400000 AS INTEGER),
        (SELECT value FROM catalog_meta WHERE key='machine'),1)
        ON CONFLICT(image_id) DO UPDATE SET modified=excluded.modified,machine=excluded.machine,dirty=1;
    UPDATE catalog_meta SET value=CAST(value AS INTEGER)+1 WHERE key='people_rev';
END;
CREATE TRIGGER IF NOT EXISTS drive_face_person_update AFTER UPDATE OF person_id,confirmed ON face_person
WHEN NOT EXISTS(SELECT 1 FROM catalog_meta WHERE key='importing') BEGIN
    UPDATE face_person SET modified=CAST((julianday('now')-2440587.5)*86400000 AS INTEGER),
        machine=(SELECT value FROM catalog_meta WHERE key='machine')
        WHERE image_id=new.image_id AND ordinal=new.ordinal;
    INSERT INTO drive_stamp(image_id,modified,machine,dirty) VALUES(new.image_id,
        CAST((julianday('now')-2440587.5)*86400000 AS INTEGER),
        (SELECT value FROM catalog_meta WHERE key='machine'),1)
        ON CONFLICT(image_id) DO UPDATE SET modified=excluded.modified,machine=excluded.machine,dirty=1;
    UPDATE catalog_meta SET value=CAST(value AS INTEGER)+1 WHERE key='people_rev';
END;
CREATE TRIGGER IF NOT EXISTS drive_face_person_delete AFTER DELETE ON face_person
WHEN NOT EXISTS(SELECT 1 FROM catalog_meta WHERE key='importing')
    AND EXISTS(SELECT 1 FROM image WHERE id=old.image_id) BEGIN
    INSERT INTO drive_stamp(image_id,modified,machine,dirty) VALUES(old.image_id,
        CAST((julianday('now')-2440587.5)*86400000 AS INTEGER),
        (SELECT value FROM catalog_meta WHERE key='machine'),1)
        ON CONFLICT(image_id) DO UPDATE SET modified=excluded.modified,machine=excluded.machine,dirty=1;
    UPDATE catalog_meta SET value=CAST(value AS INTEGER)+1 WHERE key='people_rev';
END;

-- Scores, faces and understanding: the image's analysis record.
CREATE TRIGGER IF NOT EXISTS drive_score_write AFTER INSERT ON score
WHEN NOT EXISTS(SELECT 1 FROM catalog_meta WHERE key='importing') BEGIN
    INSERT INTO drive_stamp(image_id,modified,machine,dirty) VALUES(new.image_id,
        CAST((julianday('now')-2440587.5)*86400000 AS INTEGER),
        (SELECT value FROM catalog_meta WHERE key='machine'),1)
        ON CONFLICT(image_id) DO UPDATE SET modified=excluded.modified,machine=excluded.machine,dirty=1;
END;
CREATE TRIGGER IF NOT EXISTS drive_score_update AFTER UPDATE OF value,model ON score
WHEN NOT EXISTS(SELECT 1 FROM catalog_meta WHERE key='importing') BEGIN
    INSERT INTO drive_stamp(image_id,modified,machine,dirty) VALUES(new.image_id,
        CAST((julianday('now')-2440587.5)*86400000 AS INTEGER),
        (SELECT value FROM catalog_meta WHERE key='machine'),1)
        ON CONFLICT(image_id) DO UPDATE SET modified=excluded.modified,machine=excluded.machine,dirty=1;
END;
CREATE TRIGGER IF NOT EXISTS drive_score_delete AFTER DELETE ON score
WHEN NOT EXISTS(SELECT 1 FROM catalog_meta WHERE key='importing')
    AND EXISTS(SELECT 1 FROM image WHERE id=old.image_id) BEGIN
    INSERT INTO drive_stamp(image_id,modified,machine,dirty) VALUES(old.image_id,
        CAST((julianday('now')-2440587.5)*86400000 AS INTEGER),
        (SELECT value FROM catalog_meta WHERE key='machine'),1)
        ON CONFLICT(image_id) DO UPDATE SET modified=excluded.modified,machine=excluded.machine,dirty=1;
END;
CREATE TRIGGER IF NOT EXISTS drive_face_write AFTER INSERT ON face
WHEN NOT EXISTS(SELECT 1 FROM catalog_meta WHERE key='importing') BEGIN
    INSERT INTO drive_stamp(image_id,modified,machine,dirty) VALUES(new.image_id,
        CAST((julianday('now')-2440587.5)*86400000 AS INTEGER),
        (SELECT value FROM catalog_meta WHERE key='machine'),1)
        ON CONFLICT(image_id) DO UPDATE SET modified=excluded.modified,machine=excluded.machine,dirty=1;
END;
CREATE TRIGGER IF NOT EXISTS drive_understanding_write AFTER INSERT ON understanding
WHEN NOT EXISTS(SELECT 1 FROM catalog_meta WHERE key='importing') BEGIN
    INSERT INTO drive_stamp(image_id,modified,machine,dirty) VALUES(new.image_id,
        CAST((julianday('now')-2440587.5)*86400000 AS INTEGER),
        (SELECT value FROM catalog_meta WHERE key='machine'),1)
        ON CONFLICT(image_id) DO UPDATE SET modified=excluded.modified,machine=excluded.machine,dirty=1;
END;
CREATE TRIGGER IF NOT EXISTS drive_understanding_update AFTER UPDATE ON understanding
WHEN NOT EXISTS(SELECT 1 FROM catalog_meta WHERE key='importing') BEGIN
    INSERT INTO drive_stamp(image_id,modified,machine,dirty) VALUES(new.image_id,
        CAST((julianday('now')-2440587.5)*86400000 AS INTEGER),
        (SELECT value FROM catalog_meta WHERE key='machine'),1)
        ON CONFLICT(image_id) DO UPDATE SET modified=excluded.modified,machine=excluded.machine,dirty=1;
END;
-- Removed images take their stamp with them.
CREATE TRIGGER IF NOT EXISTS drive_stamp_image_delete AFTER DELETE ON image BEGIN
    DELETE FROM drive_stamp WHERE image_id=old.id;
END;
INSERT INTO migration(version) VALUES(11);
COMMIT;

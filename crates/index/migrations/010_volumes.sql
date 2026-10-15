-- Volume-aware roots (CS-1): a root on an external volume is remembered by the
-- volume UUID and the path relative to its mount point; the last mount point is
-- only a hint. Boot-volume roots keep volume_uuid NULL and their plain path.
-- The root columns (volume_uuid, relative_path, mount_hint) are added by
-- `Core::open` first, since ALTER TABLE cannot be replayed idempotently.
BEGIN IMMEDIATE;
CREATE INDEX IF NOT EXISTS root_volume_idx ON root(volume_uuid, relative_path);
-- Small catalog facts (the image-id scheme, the machine id, stamps).
CREATE TABLE IF NOT EXISTS catalog_meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);
-- A catalog that already holds images keeps path-derived ids until the host
-- migrates them to volume-relative ids (`Index::rekey_images`); an empty one
-- starts on the new scheme.
INSERT OR IGNORE INTO catalog_meta(key,value)
    SELECT 'id_scheme', CASE WHEN EXISTS(SELECT 1 FROM image) THEN 'path' ELSE 'volume' END;
INSERT INTO migration(version) VALUES(10);
COMMIT;

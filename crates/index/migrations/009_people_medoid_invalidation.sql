BEGIN IMMEDIATE;
-- Includes cascading deletions from re-detection, pruning and image removal.
-- Keep names/identities, but never match against a representative of removed faces.
CREATE TRIGGER IF NOT EXISTS face_person_deleted_medoid
AFTER DELETE ON face_person
BEGIN
    UPDATE person SET medoid=NULL WHERE id=OLD.person_id;
END;
INSERT INTO migration(version) VALUES(9);
COMMIT;

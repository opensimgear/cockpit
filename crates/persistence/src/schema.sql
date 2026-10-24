CREATE TABLE domains (
    name TEXT PRIMARY KEY NOT NULL CHECK(length(name) BETWEEN 1 AND 4096)
) STRICT;
CREATE TABLE records (
    domain TEXT NOT NULL REFERENCES domains(name),
    key TEXT NOT NULL CHECK(length(key) BETWEEN 1 AND 4096),
    revision INTEGER NOT NULL CHECK(revision > 0),
    payload BLOB,
    PRIMARY KEY(domain,key)
) STRICT;

-- Incarnation identity, not a schema version. Restore rotates it atomically.
CREATE TABLE incarnation (singleton INTEGER PRIMARY KEY CHECK(singleton=1), epoch TEXT NOT NULL CHECK(length(epoch)=32)) STRICT;
INSERT INTO incarnation VALUES(1,lower(hex(randomblob(16))));

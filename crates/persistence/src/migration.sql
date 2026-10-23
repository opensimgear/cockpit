CREATE TABLE migration_state (
    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
    generation INTEGER NOT NULL CHECK(generation >= 0),
    ready INTEGER NOT NULL CHECK(ready IN (0,1)),
    backup_path TEXT
) STRICT;
INSERT INTO migration_state VALUES(1,0,0,NULL);
CREATE TABLE migration_requirements (
    origin TEXT PRIMARY KEY NOT NULL
) STRICT;
CREATE TABLE migration_origins (
    origin TEXT PRIMARY KEY NOT NULL REFERENCES migration_requirements(origin),
    checksum TEXT NOT NULL
) STRICT;
CREATE TABLE migration_snapshots (
    origin TEXT NOT NULL REFERENCES migration_origins(origin),
    source TEXT NOT NULL,
    domain TEXT NOT NULL,
    key TEXT NOT NULL,
    format_version INTEGER NOT NULL CHECK(format_version>0),
    checksum TEXT,
    payload BLOB,
    PRIMARY KEY(origin,source),
    UNIQUE(origin,domain,key),
    CHECK((checksum IS NULL)=(payload IS NULL))
) STRICT;

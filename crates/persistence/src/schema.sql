CREATE TABLE IF NOT EXISTS domains (
    name TEXT PRIMARY KEY NOT NULL CHECK(length(name) BETWEEN 1 AND 4096)
) STRICT;
CREATE TABLE IF NOT EXISTS records (
    domain TEXT NOT NULL REFERENCES domains(name),
    key TEXT NOT NULL CHECK(length(key) BETWEEN 1 AND 4096),
    format_version INTEGER NOT NULL CHECK(format_version > 0),
    revision INTEGER NOT NULL CHECK(revision > 0),
    payload BLOB,
    PRIMARY KEY(domain,key)
) STRICT;
CREATE TABLE IF NOT EXISTS imports (
    origin TEXT NOT NULL,
    source TEXT NOT NULL,
    domain TEXT NOT NULL REFERENCES domains(name),
    key TEXT NOT NULL,
    format_version INTEGER NOT NULL CHECK(format_version > 0),
    checksum TEXT,
    snapshot BLOB,
    PRIMARY KEY(origin,source),
    CHECK((checksum IS NULL) = (snapshot IS NULL))
) STRICT;
CREATE TABLE IF NOT EXISTS origins (
    name TEXT PRIMARY KEY NOT NULL
) STRICT;

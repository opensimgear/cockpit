CREATE TABLE IF NOT EXISTS activity_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    occurred_at INTEGER NOT NULL,
    category TEXT NOT NULL,
    event_type TEXT NOT NULL,
    level TEXT NOT NULL,
    message TEXT NOT NULL,
    detail TEXT NOT NULL,
    entity_id TEXT,
    entity_name TEXT,
    source TEXT NOT NULL
) STRICT;
CREATE INDEX IF NOT EXISTS activity_time ON activity_events(occurred_at DESC,id DESC);
CREATE INDEX IF NOT EXISTS activity_entity ON activity_events(entity_id);
CREATE TABLE IF NOT EXISTS activity_identity (
    event_key TEXT PRIMARY KEY,
    event_id INTEGER NOT NULL UNIQUE REFERENCES activity_events(id) ON DELETE CASCADE
) STRICT;
CREATE VIRTUAL TABLE IF NOT EXISTS activity_fts USING fts5(message,detail,entity_name,entity_id,event_type,category, tokenize='unicode61');
CREATE TABLE IF NOT EXISTS activity_settings (
    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
    retention_days INTEGER NOT NULL CHECK(retention_days BETWEEN 1 AND 3650)
) STRICT;
INSERT INTO activity_settings VALUES(1,7) ON CONFLICT DO NOTHING;

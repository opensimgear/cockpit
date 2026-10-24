-- Derived profile indexes; source documents and indexes commit together.
CREATE TABLE profile_documents (
 id TEXT PRIMARY KEY NOT NULL,
 owner_domain TEXT NOT NULL DEFAULT 'profiles', owner_key TEXT NOT NULL DEFAULT 'library',
 document_revision INTEGER NOT NULL CHECK(document_revision>0), name TEXT NOT NULL, document BLOB NOT NULL,
 FOREIGN KEY(owner_domain,owner_key) REFERENCES records(domain,key)
) STRICT;
CREATE TABLE profile_local_assignments (
 profile_id TEXT NOT NULL REFERENCES profile_documents(id) ON DELETE CASCADE,
 resource_id TEXT NOT NULL, source_id TEXT NOT NULL, ports BLOB NOT NULL,
 PRIMARY KEY(profile_id,resource_id)
) STRICT;
CREATE INDEX profile_assignment_source ON profile_local_assignments(source_id);
CREATE TABLE profile_game_references (
 profile_id TEXT NOT NULL REFERENCES profile_documents(id) ON DELETE CASCADE,
 game_id TEXT NOT NULL, PRIMARY KEY(profile_id,game_id)
) STRICT;
CREATE INDEX profile_reference_game ON profile_game_references(game_id);
CREATE TABLE profile_companion_references (
 profile_id TEXT NOT NULL REFERENCES profile_documents(id) ON DELETE CASCADE,
 resource_id TEXT NOT NULL, panel_id TEXT, publication_hash TEXT NOT NULL,
 PRIMARY KEY(profile_id,resource_id)
) STRICT;
CREATE INDEX profile_reference_panel ON profile_companion_references(panel_id);
CREATE INDEX profile_reference_publication ON profile_companion_references(publication_hash);

CREATE TRIGGER profile_library_insert AFTER INSERT ON records
WHEN NEW.domain='profiles' AND NEW.key='library'
BEGIN
 DELETE FROM profile_documents WHERE owner_domain=NEW.domain AND owner_key=NEW.key;
 INSERT INTO profile_documents(id,document_revision,name,document)
 SELECT json_extract(p.value,'$.id'),json_extract(p.value,'$.revision'),json_extract(p.value,'$.name'),CAST(p.value AS BLOB)
 FROM json_each(CASE WHEN json_valid(CAST(NEW.payload AS TEXT)) THEN CAST(NEW.payload AS TEXT) ELSE '{}' END,'$.profiles') p;
 INSERT INTO profile_local_assignments(profile_id,resource_id,source_id,ports)
 SELECT assignment_groups.key,json_extract(a.value,'$.resourceId'),json_extract(a.value,'$.sourceId'),CAST(json_extract(a.value,'$.ports') AS BLOB)
 FROM json_each(CASE WHEN json_valid(CAST(NEW.payload AS TEXT)) THEN CAST(NEW.payload AS TEXT) ELSE '{}' END,'$.localAssignments') assignment_groups,json_each(assignment_groups.value) a;
 INSERT INTO profile_game_references(profile_id,game_id)
 SELECT p.id,g.value FROM profile_documents p,json_each(CAST(p.document AS TEXT),'$.eligibility.gameIds') g;
 INSERT INTO profile_companion_references(profile_id,resource_id,panel_id,publication_hash)
 SELECT p.id,json_extract(a.value,'$.resourceId'),json_extract(a.value,'$.panelId'),json_extract(a.value,'$.publicationHash')
 FROM profile_documents p,json_each(CAST(p.document AS TEXT),'$.companionAssignments') a;
END;

CREATE TRIGGER profile_library_update AFTER UPDATE OF payload ON records
WHEN NEW.domain='profiles' AND NEW.key='library'
BEGIN
 DELETE FROM profile_documents WHERE owner_domain=NEW.domain AND owner_key=NEW.key;
 INSERT INTO profile_documents(id,document_revision,name,document)
 SELECT json_extract(p.value,'$.id'),json_extract(p.value,'$.revision'),json_extract(p.value,'$.name'),CAST(p.value AS BLOB)
 FROM json_each(CASE WHEN json_valid(CAST(NEW.payload AS TEXT)) THEN CAST(NEW.payload AS TEXT) ELSE '{}' END,'$.profiles') p;
 INSERT INTO profile_local_assignments(profile_id,resource_id,source_id,ports)
 SELECT assignment_groups.key,json_extract(a.value,'$.resourceId'),json_extract(a.value,'$.sourceId'),CAST(json_extract(a.value,'$.ports') AS BLOB)
 FROM json_each(CASE WHEN json_valid(CAST(NEW.payload AS TEXT)) THEN CAST(NEW.payload AS TEXT) ELSE '{}' END,'$.localAssignments') assignment_groups,json_each(assignment_groups.value) a;
 INSERT INTO profile_game_references(profile_id,game_id)
 SELECT p.id,g.value FROM profile_documents p,json_each(CAST(p.document AS TEXT),'$.eligibility.gameIds') g;
 INSERT INTO profile_companion_references(profile_id,resource_id,panel_id,publication_hash)
 SELECT p.id,json_extract(a.value,'$.resourceId'),json_extract(a.value,'$.panelId'),json_extract(a.value,'$.publicationHash')
 FROM profile_documents p,json_each(CAST(p.document AS TEXT),'$.companionAssignments') a;
END;

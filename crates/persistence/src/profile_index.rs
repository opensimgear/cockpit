//! Queryable saved identities and soft references. These never prove live
//! resources, running processes or an applied profile; domain validation and
//! runtime ownership remain required at activation.
use crate::{failure, identifier, Database};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileIndex {
    pub id: String,
    pub revision: u64,
    pub name: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileReference {
    pub kind: String,
    pub target: String,
    pub resource_id: Option<String>,
}
impl Database {
    pub fn profile_index(&self) -> Result<Vec<ProfileIndex>, String> {
        let connection = self.connection()?;
        let mut query = connection
            .prepare("SELECT id,document_revision,name FROM profile_documents ORDER BY id LIMIT 33")
            .map_err(failure)?;
        let profiles = query
            .query_map([], |row| {
                Ok(ProfileIndex {
                    id: row.get(0)?,
                    revision: row.get::<_, i64>(1)? as u64,
                    name: row.get(2)?,
                })
            })
            .map_err(failure)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(failure)?;
        if profiles.len() > 32 {
            return Err(
                "Profile index exceeds its domain bound. Preserve the database for recovery."
                    .into(),
            );
        }
        Ok(profiles)
    }
    /// Missing game/controller/companion targets remain readable soft references.
    /// Forgetting a device or uninstalling a game cannot cascade-delete a graph.
    pub fn profile_references(&self, id: &str) -> Result<Vec<ProfileReference>, String> {
        identifier(id)?;
        let connection = self.connection()?;
        let mut query=connection.prepare("SELECT 'source',source_id,resource_id FROM profile_local_assignments WHERE profile_id=?1
            UNION ALL SELECT 'game',game_id,NULL FROM profile_game_references WHERE profile_id=?1
            UNION ALL SELECT 'panel',panel_id,resource_id FROM profile_companion_references WHERE profile_id=?1 AND panel_id IS NOT NULL
            UNION ALL SELECT 'publication',publication_hash,resource_id FROM profile_companion_references WHERE profile_id=?1
            ORDER BY 1,2 LIMIT 4097").map_err(failure)?;
        let references = query
            .query_map([id], |row| {
                Ok(ProfileReference {
                    kind: row.get(0)?,
                    target: row.get(1)?,
                    resource_id: row.get(2)?,
                })
            })
            .map_err(failure)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(failure)?;
        if references.len() > 4096 {
            return Err("Profile references exceed their domain bound.".into());
        }
        Ok(references)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Change;
    const ID: &str = "00000000-0000-4000-8000-000000000001";
    fn library() -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({"schemaVersion":2,"profiles":[{"id":ID,"name":"Default","revision":7,
            "eligibility":{"gameIds":["dcs"]},"companionAssignments":[{"resourceId":"panel","panelId":"panel-one","publicationHash":format!("sha256:{}","a".repeat(64))}]}],
            "localAssignments":{ID:[{"resourceId":"stick","sourceId":"hid:missing","ports":[]}]}})).unwrap()
    }
    #[test]
    fn indexes_identities_revisions_and_missing_targets_with_the_document_commit() {
        let dir = crate::test_directory();
        let db = Database::open(dir.path().join("cockpit.sqlite3")).unwrap();
        let payload = library();
        db.commit(&[Change {
            domain: "profiles",
            key: "library",
            expected_revision: 0,
            payload: Some(&payload),
        }])
        .unwrap();
        assert_eq!(
            db.profile_index().unwrap(),
            vec![ProfileIndex {
                id: ID.into(),
                name: "Default".into(),
                revision: 7
            }]
        );
        let refs = db.profile_references(ID).unwrap();
        assert_eq!(refs.len(), 4);
        assert!(refs
            .iter()
            .any(|r| r.kind == "source" && r.target == "hid:missing"));
        assert!(refs.iter().any(|r| r.kind == "game" && r.target == "dcs"));
        assert!(refs
            .iter()
            .any(|r| r.kind == "panel" && r.target == "panel-one"));
        assert!(db.profile_references("").is_err());
        db.integrity_check().unwrap();
    }
    #[test]
    fn stale_or_failed_multi_domain_save_cannot_change_profile_indexes_or_references() {
        let dir = crate::test_directory();
        let db = Database::open(dir.path().join("cockpit.sqlite3")).unwrap();
        let payload = library();
        db.commit(&[Change {
            domain: "profiles",
            key: "library",
            expected_revision: 0,
            payload: Some(&payload),
        }])
        .unwrap();
        let mut changed: serde_json::Value = serde_json::from_slice(&payload).unwrap();
        changed["profiles"][0]["revision"] = 8.into();
        changed["profiles"][0]["eligibility"]["gameIds"] = serde_json::json!([]);
        let edited = serde_json::to_vec(&changed).unwrap();
        let before = db.profile_references(ID).unwrap();
        assert!(db
            .commit(&[
                Change {
                    domain: "profiles",
                    key: "library",
                    expected_revision: 1,
                    payload: Some(&edited)
                },
                Change {
                    domain: "settings",
                    key: "conflict",
                    expected_revision: 4,
                    payload: Some(b"{}")
                }
            ])
            .is_err());
        assert_eq!(db.profile_index().unwrap()[0].revision, 7);
        assert_eq!(db.profile_references(ID).unwrap(), before);
        db.commit(&[Change {
            domain: "profiles",
            key: "library",
            expected_revision: 1,
            payload: Some(&edited),
        }])
        .unwrap();
        assert_eq!(db.profile_index().unwrap()[0].revision, 8);
        assert!(!db
            .profile_references(ID)
            .unwrap()
            .iter()
            .any(|r| r.kind == "game"));
        db.commit(&[Change {
            domain: "profiles",
            key: "library",
            expected_revision: 2,
            payload: None,
        }])
        .unwrap();
        assert!(db.profile_index().unwrap().is_empty());
        assert!(db.profile_references(ID).unwrap().is_empty());
    }
    #[test]
    fn orphan_assignments_rollback_and_untrusted_index_sizes_are_bounded() {
        let dir = crate::test_directory();
        let db = Database::open(dir.path().join("cockpit.sqlite3")).unwrap();
        let payload = library();
        db.commit(&[Change {
            domain: "profiles",
            key: "library",
            expected_revision: 0,
            payload: Some(&payload),
        }])
        .unwrap();
        let mut corrupt: serde_json::Value = serde_json::from_slice(&payload).unwrap();
        corrupt["localAssignments"]["missing-profile"] =
            serde_json::json!([{"resourceId":"stick","sourceId":"hid:missing","ports":[]}]);
        let orphan = serde_json::to_vec(&corrupt).unwrap();
        assert!(db
            .commit(&[Change {
                domain: "profiles",
                key: "library",
                expected_revision: 1,
                payload: Some(&orphan)
            }])
            .is_err());
        assert_eq!(
            db.read("profiles", "library").unwrap().unwrap().payload,
            payload
        );
        assert_eq!(db.profile_index().unwrap()[0].revision, 7);
        let mut excessive: serde_json::Value = serde_json::from_slice(&payload).unwrap();
        excessive["profiles"][0]["eligibility"]["gameIds"] =
            serde_json::json!((0..4097).map(|i| format!("game-{i}")).collect::<Vec<_>>());
        let bytes = serde_json::to_vec(&excessive).unwrap();
        db.commit(&[Change {
            domain: "profiles",
            key: "library",
            expected_revision: 1,
            payload: Some(&bytes),
        }])
        .unwrap();
        assert!(db.profile_references(ID).is_err());
        excessive["profiles"] = serde_json::json!((0..33)
            .map(|i| serde_json::json!({"id":format!("profile-{i}"),"name":"test","revision":1}))
            .collect::<Vec<_>>());
        excessive["localAssignments"] = serde_json::json!({});
        let bytes = serde_json::to_vec(&excessive).unwrap();
        db.commit(&[Change {
            domain: "profiles",
            key: "library",
            expected_revision: 2,
            payload: Some(&bytes),
        }])
        .unwrap();
        assert!(db.profile_index().is_err());
    }
}

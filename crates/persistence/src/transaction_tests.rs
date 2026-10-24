use super::*;
#[test]
fn related_snapshots_include_tombstones_and_reference_checks_do_not_rewrite_their_owner(){
    let dir=crate::test_directory();let db=Database::open(dir.path().join("cockpit.sqlite3")).unwrap();
    let keys=[("authority","secret"),("content","publications")];
    let missing=db.read_many(&keys).unwrap();assert!(missing.iter().all(|(revision,record)|*revision==0&&record.is_none()));
    let changes=[Change{domain:"authority",key:"secret",expected_revision:0,payload:Some(b"ciphertext")},Change{domain:"content",key:"publications",expected_revision:0,payload:Some(b"first")}];
    db.commit_checked(&changes,&[RevisionCheck{domain:"references",key:"profiles",expected_revision:0}]).unwrap();
    let snapshot=db.read_many(&keys).unwrap();assert!(snapshot.iter().all(|(revision,_)|*revision==1));assert_eq!(snapshot[0].1.as_ref().unwrap().payload,b"ciphertext");assert_eq!(db.revision("references","profiles").unwrap(),0);
    db.commit(&[Change{domain:"references",key:"profiles",expected_revision:0,payload:Some(b"new reference")}]).unwrap();
    assert!(db.commit_checked(&[Change{domain:"content",key:"publications",expected_revision:1,payload:None}],&[RevisionCheck{domain:"references",key:"profiles",expected_revision:0}]).unwrap_err().contains("reference revision conflict"));
    assert_eq!(db.read("content","publications").unwrap().unwrap().payload,b"first");
    db.commit_checked(&[Change{domain:"content",key:"publications",expected_revision:1,payload:None}],&[RevisionCheck{domain:"references",key:"profiles",expected_revision:1}]).unwrap();
    let snapshot=db.read_many(&keys).unwrap();assert_eq!(snapshot[1].0,2);assert!(snapshot[1].1.is_none());assert_eq!(db.revision("references","profiles").unwrap(),1);
    assert!(db.read_many(&[]).is_err());assert!(db.read_many(&[("","bad")]).is_err());assert!(db.commit_checked(&[],&[]).is_err());
    assert!(db.commit_checked(&[Change{domain:"content",key:"publications",expected_revision:2,payload:None}],&[RevisionCheck{domain:"",key:"bad",expected_revision:0}]).is_err());
}

#[test]
fn concurrent_related_writes_are_never_observed_as_a_mixed_snapshot(){
    let dir=crate::test_directory();let db=Database::open(dir.path().join("cockpit.sqlite3")).unwrap();let writer=db.clone();
    let barrier=std::sync::Arc::new(std::sync::Barrier::new(2));let ready=barrier.clone();
    let task=std::thread::spawn(move||{ready.wait();for expected in 0u64..20 {
        let bytes=expected.to_le_bytes();writer.commit(&[Change{domain:"pair",key:"sealed",expected_revision:expected,payload:Some(&bytes)},Change{domain:"pair",key:"content",expected_revision:expected,payload:Some(&bytes)}]).unwrap();
    }});
    barrier.wait();for _ in 0..30 {let snapshot=db.read_many(&[("pair","sealed"),("pair","content")]).unwrap();assert_eq!(snapshot[0].0,snapshot[1].0);assert_eq!(snapshot[0].1.as_ref().map(|record|&record.payload),snapshot[1].1.as_ref().map(|record|&record.payload));}
    task.join().unwrap();assert_eq!(db.revision("pair","sealed").unwrap(),20);
}

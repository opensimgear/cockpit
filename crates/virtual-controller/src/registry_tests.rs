use super::*;
struct Fixture {
    key: Key,
    path: String,
}
impl Fixture {
    fn new() -> Self {
        let path = format!("Software\\OpenSimGearTest\\{}", uuid::Uuid::new_v4());
        let (key, created) = Key::create(HKEY_CURRENT_USER, &path, None).unwrap();
        assert!(created);
        Self { key, path }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        assert!(self.path.starts_with("Software\\OpenSimGearTest\\"));
        assert!(uuid::Uuid::parse_str(self.path.rsplit('\\').next().unwrap()).is_ok());
        unsafe {
            RegDeleteTreeW(HKEY_CURRENT_USER, wide(&self.path).unwrap().as_ptr());
        }
    }
}
#[test]
fn actual_registry_transactions_commit_creation_and_ownership_together_or_roll_back() {
    let f = Fixture::new();
    {
        let tx = Transaction::new().unwrap();
        let (device, new) = match Key::create(f.key.0, "Device01", Some(&tx)) {
            Ok(created) => created,
            Err(error) if error.contains("os error 6801") => {
                // Packaged Windows processes can have a private HKCU hive without
                // a transaction resource manager. No nontransactional fallback.
                eprintln!("UNQUALIFIED: HKCU registry transactions unavailable: {error}");
                assert!(Key::open(f.key.0, "Device01", KEY_READ, None)
                    .unwrap()
                    .is_none());
                assert!(Key::open(f.key.0, "Journal", KEY_READ, None)
                    .unwrap()
                    .is_none());
                return;
            }
            Err(error) => panic!("{error}"),
        };
        assert!(new);
        device.write("Descriptor", REG_BINARY, &[1, 2, 3]).unwrap();
        let (journal, new) = Key::create(f.key.0, "Journal", Some(&tx)).unwrap();
        assert!(new);
        journal
            .write("Owned", REG_DWORD, &1u32.to_le_bytes())
            .unwrap();
        // Dropping the final transaction handle rolls back both keys.
    }
    assert!(Key::open(f.key.0, "Device01", KEY_READ, None)
        .unwrap()
        .is_none());
    assert!(Key::open(f.key.0, "Journal", KEY_READ, None)
        .unwrap()
        .is_none());
    let tx = Transaction::new().unwrap();
    {
        let (device, new) = Key::create(f.key.0, "Device01", Some(&tx)).unwrap();
        assert!(new);
        device.write("Descriptor", REG_BINARY, &[1, 2, 3]).unwrap();
        let (journal, new) = Key::create(f.key.0, "Journal", Some(&tx)).unwrap();
        assert!(new);
        journal
            .write("Owned", REG_DWORD, &1u32.to_le_bytes())
            .unwrap();
    }
    tx.commit().unwrap();
    let device = Key::open(f.key.0, "Device01", KEY_READ, None)
        .unwrap()
        .unwrap();
    assert_eq!(device.value("Descriptor").unwrap().unwrap().data, [1, 2, 3]);
    let tx = Transaction::new().unwrap();
    let (existing, new) = Key::create(f.key.0, "Device01", Some(&tx)).unwrap();
    assert!(
        !new,
        "An external key must never be mistaken for exclusive creation."
    );
    drop(existing);
    drop(tx);
    assert_eq!(device.value("Descriptor").unwrap().unwrap().data, [1, 2, 3]);
    let tx = Transaction::new().unwrap();
    {
        let journal = Key::open(f.key.0, "Journal", KEY_READ | KEY_WRITE, Some(&tx))
            .unwrap()
            .unwrap();
        journal
            .write("Owned", REG_DWORD, &0u32.to_le_bytes())
            .unwrap();
        f.key.delete_child("Device01", &tx).unwrap();
    }
    tx.commit().unwrap();
    assert!(Key::open(f.key.0, "Device01", KEY_READ, None)
        .unwrap()
        .is_none());
    let journal = Key::open(f.key.0, "Journal", KEY_READ, None)
        .unwrap()
        .unwrap();
    assert_eq!(journal.value("Owned").unwrap().unwrap().data, [0, 0, 0, 0]);
    let tx = Transaction::new().unwrap();
    assert!(Key::open(f.key.0, "Missing", KEY_READ, Some(&tx))
        .unwrap()
        .is_none());
    f.key.delete_child("Missing", &tx).unwrap();
    tx.commit().unwrap();
}
#[test]
fn actual_registry_fingerprints_preserve_types_and_bound_names_values_and_trees() {
    let f = Fixture::new();
    let (child, _) = Key::create(f.key.0, "Child", None).unwrap();
    f.key.write("Binary", REG_BINARY, &[0, 255, 3]).unwrap();
    f.key
        .write("Size", REG_DWORD, &123u32.to_le_bytes())
        .unwrap();
    f.key.write("Empty", REG_BINARY, &[]).unwrap();
    child.write("OEM", REG_SZ, &[65, 0, 0, 0]).unwrap();
    f.key.flush().unwrap();
    assert_eq!(f.key.names(true).unwrap(), ["Child"]);
    assert_eq!(f.key.names(false).unwrap().len(), 3);
    assert!(f.key.value("Missing").unwrap().is_none());
    let before = tree(&f.key, 0, &mut 262144).unwrap();
    assert_eq!(before, tree(&f.key, 0, &mut 262144).unwrap());
    assert!(tree(&f.key, 9, &mut 262144).is_err());
    assert!(tree(&f.key, 0, &mut 63).is_err());
    assert!(tree(&f.key, 0, &mut 65).is_err());
    child.write("OEM", REG_BINARY, &[65, 0, 0, 0]).unwrap();
    assert_ne!(before, tree(&f.key, 0, &mut 262144).unwrap());
    assert!(wide("nul\0tail").is_err());
    assert!(wide(&"a".repeat(1025)).is_err());
    assert!(f
        .key
        .write("too-big", REG_BINARY, &vec![0; VALUE_LIMIT + 1])
        .is_err());
    let huge = vec![0; VALUE_LIMIT + 1];
    checked(unsafe {
        RegSetValueExW(
            f.key.0,
            wide("big").unwrap().as_ptr(),
            0,
            REG_BINARY,
            huge.as_ptr(),
            huge.len() as u32,
        )
    })
    .unwrap();
    assert!(f.key.value("big").is_err());
    assert!(tree(&f.key, 0, &mut 262144).is_err());
    let invalid = Key(ptr::null_mut());
    assert!(invalid.names(false).is_err());
    assert!(invalid.value("value").is_err());
    assert!(Key::open(invalid.0, "bad", KEY_READ, None).is_err());
    assert!(Key::create(invalid.0, "bad", None).is_err());
    let tx = Transaction::new().unwrap();
    assert!(invalid.delete_child("bad", &tx).is_err());
    assert!(Transaction(ptr::null_mut()).commit().is_err());
}
#[test]
fn enumeration_cannot_grow_without_bounds() {
    let f = Fixture::new();
    for n in 0..257 {
        f.key
            .write(&format!("value-{n}"), REG_DWORD, &0u32.to_le_bytes())
            .unwrap();
    }
    assert!(f.key.names(false).is_err());
    let (child, _) = Key::create(f.key.0, "child", None).unwrap();
    child.write(&"n".repeat(300), REG_BINARY, &[]).unwrap();
    assert!(child.names(false).is_err());
}

#[test]
fn system_hive_transaction_can_be_checked_without_mutation() {
    let tx = Transaction::new().unwrap();
    let key = Key::open(HKEY_LOCAL_MACHINE, "SYSTEM", KEY_READ, Some(&tx))
        .unwrap()
        .unwrap();
    assert!(!key.names(true).unwrap().is_empty());
    drop(key);
    tx.commit().unwrap();
}

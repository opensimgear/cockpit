//! Bounded registry access and atomic ownership updates for the elevated helper.
//! Every transaction is local and aborts after five seconds if not committed.
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, ptr};
use windows_sys::Win32::{
    Foundation::{
        CloseHandle, ERROR_FILE_NOT_FOUND, ERROR_NO_MORE_ITEMS, HANDLE, INVALID_HANDLE_VALUE,
    },
    Storage::FileSystem::{CommitTransaction, CreateTransaction},
    System::Registry::*,
};
const VALUE_LIMIT: usize = 65_536;
pub(crate) fn wide(text: &str) -> Result<Vec<u16>, String> {
    if text.contains('\0') || text.len() > 1024 {
        return Err("Invalid registry name.".into());
    }
    Ok(text.encode_utf16().chain(Some(0)).collect())
}
fn checked(code: u32) -> Result<(), String> {
    if code == 0 {
        Ok(())
    } else {
        Err(format!(
            "Registry operation failed: {}.",
            std::io::Error::from_raw_os_error(code as i32)
        ))
    }
}
pub(crate) struct Transaction(HANDLE);
impl Transaction {
    pub fn new() -> Result<Self, String> {
        let handle = unsafe {
            CreateTransaction(ptr::null_mut(), ptr::null_mut(), 1, 0, 0, 5000, ptr::null())
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(format!(
                "Registry transactions are unavailable: {}.",
                std::io::Error::last_os_error()
            ));
        }
        Ok(Self(handle))
    }
    pub fn commit(self) -> Result<(), String> {
        if unsafe { CommitTransaction(self.0) } == 0 {
            return Err(format!(
                "Registry transaction was not committed: {}.",
                std::io::Error::last_os_error()
            ));
        }
        Ok(())
    }
}
impl Drop for Transaction {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
pub(crate) struct Key(pub HKEY);
impl Drop for Key {
    fn drop(&mut self) {
        unsafe {
            RegCloseKey(self.0);
        }
    }
}
impl Key {
    pub fn open(
        hive: HKEY,
        path: &str,
        access: u32,
        transaction: Option<&Transaction>,
    ) -> Result<Option<Self>, String> {
        let path = wide(path)?;
        let mut key = ptr::null_mut();
        let result = unsafe {
            match transaction {
                Some(tx) => RegOpenKeyTransactedW(
                    hive,
                    path.as_ptr(),
                    0,
                    access | KEY_WOW64_64KEY,
                    &mut key,
                    tx.0,
                    ptr::null(),
                ),
                None => RegOpenKeyExW(hive, path.as_ptr(), 0, access | KEY_WOW64_64KEY, &mut key),
            }
        };
        if result == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        checked(result)?;
        Ok(Some(Self(key)))
    }
    pub fn create(
        hive: HKEY,
        path: &str,
        transaction: Option<&Transaction>,
    ) -> Result<(Self, bool), String> {
        let path = wide(path)?;
        let mut key = ptr::null_mut();
        let mut disposition = 0;
        let result = unsafe {
            match transaction {
                Some(tx) => RegCreateKeyTransactedW(
                    hive,
                    path.as_ptr(),
                    0,
                    ptr::null(),
                    0,
                    KEY_READ | KEY_WRITE | KEY_WOW64_64KEY,
                    ptr::null(),
                    &mut key,
                    &mut disposition,
                    tx.0,
                    ptr::null(),
                ),
                None => RegCreateKeyExW(
                    hive,
                    path.as_ptr(),
                    0,
                    ptr::null(),
                    0,
                    KEY_READ | KEY_WRITE | KEY_WOW64_64KEY,
                    ptr::null(),
                    &mut key,
                    &mut disposition,
                ),
            }
        };
        checked(result)?;
        Ok((Self(key), disposition == REG_CREATED_NEW_KEY))
    }
    pub fn write(&self, name: &str, kind: u32, bytes: &[u8]) -> Result<(), String> {
        if bytes.len() > VALUE_LIMIT {
            return Err("Registry value exceeds 64 KiB.".into());
        }
        let name = wide(name)?;
        checked(unsafe {
            RegSetValueExW(
                self.0,
                name.as_ptr(),
                0,
                kind,
                bytes.as_ptr(),
                bytes.len() as u32,
            )
        })
    }
    pub fn value(&self, name: &str) -> Result<Option<Value>, String> {
        let name = wide(name)?;
        let mut size = 0;
        let mut kind = 0;
        let result = unsafe {
            RegQueryValueExW(
                self.0,
                name.as_ptr(),
                ptr::null(),
                &mut kind,
                ptr::null_mut(),
                &mut size,
            )
        };
        if result == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        checked(result)?;
        if size as usize > VALUE_LIMIT {
            return Err("Registry value exceeds 64 KiB.".into());
        }
        let mut data = vec![0; size as usize];
        checked(unsafe {
            RegQueryValueExW(
                self.0,
                name.as_ptr(),
                ptr::null(),
                &mut kind,
                data.as_mut_ptr(),
                &mut size,
            )
        })?;
        data.truncate(size as usize);
        Ok(Some(Value { kind, data }))
    }
    pub fn names(&self, children: bool) -> Result<Vec<String>, String> {
        let mut names = Vec::new();
        for index in 0..=256 {
            let mut buffer = [0u16; 256];
            let mut count = buffer.len() as u32;
            let result = unsafe {
                if children {
                    RegEnumKeyExW(
                        self.0,
                        index,
                        buffer.as_mut_ptr(),
                        &mut count,
                        ptr::null(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                    )
                } else {
                    RegEnumValueW(
                        self.0,
                        index,
                        buffer.as_mut_ptr(),
                        &mut count,
                        ptr::null(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                    )
                }
            };
            if result == ERROR_NO_MORE_ITEMS {
                return Ok(names);
            }
            checked(result)?;
            if index == 256 {
                return Err("Registry key exceeds 256 entries.".into());
            }
            names.push(
                String::from_utf16(&buffer[..count as usize])
                    .map_err(|_| "Invalid registry name encoding.")?,
            );
        }
        unreachable!()
    }
    pub fn flush(&self) -> Result<(), String> {
        checked(unsafe { RegFlushKey(self.0) })
    }
    pub fn delete_child(&self, name: &str, tx: &Transaction) -> Result<(), String> {
        let name = wide(name)?;
        let result = unsafe {
            RegDeleteKeyTransactedW(self.0, name.as_ptr(), KEY_WOW64_64KEY, 0, tx.0, ptr::null())
        };
        if result == ERROR_FILE_NOT_FOUND {
            return Ok(());
        }
        checked(result)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Value {
    pub kind: u32,
    pub data: Vec<u8>,
}
#[derive(Debug, PartialEq, Eq, Serialize)]
pub(crate) struct Tree {
    values: BTreeMap<String, Value>,
    children: BTreeMap<String, Tree>,
}
pub(crate) fn tree(key: &Key, depth: usize, budget: &mut usize) -> Result<Tree, String> {
    if depth > 8 || *budget < 64 {
        return Err("Registry tree exceeds its preservation bound.".into());
    }
    *budget -= 64;
    let mut values = BTreeMap::new();
    for name in key.names(false)? {
        let value = key
            .value(&name)?
            .ok_or("Registry changed while reading its fingerprint.")?;
        let size = name.len() + value.data.len() + 16;
        *budget = budget
            .checked_sub(size)
            .ok_or("Registry tree exceeds its preservation bound.")?;
        values.insert(name.to_ascii_lowercase(), value);
    }
    let mut children = BTreeMap::new();
    for name in key.names(true)? {
        let child = Key::open(key.0, &name, KEY_READ, None)?
            .ok_or("Registry changed while reading its fingerprint.")?;
        children.insert(name.to_ascii_lowercase(), tree(&child, depth + 1, budget)?);
    }
    Ok(Tree { values, children })
}
#[cfg(test)]
#[path = "registry_tests.rs"]
mod tests;

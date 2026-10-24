//! OS-protected bytes enter SQLite only after sealing. Retired leaf files are ignored.
use super::{StateStorage,STATE_BYTES};
use osg_persistence::{Change,Database};
use std::{path::PathBuf,sync::{Arc,Mutex,OnceLock}};
use zeroize::Zeroize;
/// Native OS protection adapter. This interface is never granted to JavaScript;
/// the shared SQLite owner always handles revisions and durable ciphertext.
pub trait NativeSealer:Send+Sync {
    fn available(&self)->Result<(),String>;
    fn seal(&self,plain:&[u8])->Result<Vec<u8>,String>;
    fn unseal(&self,cipher:&[u8])->Result<Vec<u8>,String>;
}
struct PlatformSealer;
impl NativeSealer for PlatformSealer {
    fn available(&self)->Result<(),String>{super::available()}
    fn seal(&self,plain:&[u8])->Result<Vec<u8>,String>{super::seal(plain)}
    fn unseal(&self,cipher:&[u8])->Result<Vec<u8>,String>{super::unseal(cipher)}
}
#[derive(Clone)]
pub struct SealedSqliteStore(Arc<Inner>);
struct Inner {
    anchor:PathBuf,
    database:OnceLock<Database>,
    revision:Mutex<Option<u64>>,
    content_revision:Mutex<Option<u64>>,
    sealer:Arc<dyn NativeSealer>,
}
impl SealedSqliteStore {
    pub fn new(anchor:impl Into<PathBuf>)->Self {
        Self::with_sealer(anchor,Arc::new(PlatformSealer))
    }
    pub fn with_sealer(anchor:impl Into<PathBuf>,sealer:Arc<dyn NativeSealer>)->Self {
        Self(Arc::new(Inner{anchor:anchor.into(),database:OnceLock::new(),revision:Mutex::new(None),content_revision:Mutex::new(None),sealer}))
    }
    fn key(&self)->Result<&str,String>{
        let name=self.0.anchor.file_name().and_then(|s|s.to_str()).ok_or("Invalid protected-state identity.")?;
        Ok(match name {"companion-host-state.dpapi"=>"companion-host","companion-identity.dpapi"=>"companion-identity","companion-client.dpapi"=>"companion-client",name=>name})
    }
    fn database(&self,create:bool)->Result<Option<&Database>,String>{
        if self.0.database.get().is_none(){
            let database=if create {Some(Database::for_namespace(&self.0.anchor)?)}else{Database::existing_for_namespace(&self.0.anchor)?};
            if let Some(database)=database{let _=self.0.database.set(database);}
        }
        Ok(self.0.database.get())
    }
    /// Discovery inspects presence only, without decrypting or creating keys.
    pub fn exists(&self)->Result<bool,String>{
        let Some(database)=self.database(false)? else{return Ok(false)};
        Ok(database.read("secrets",self.key()?)?.is_some())
    }
    fn read_record(&self,database:&Database)->Result<(u64,Option<Vec<u8>>),String>{
        let(revision,record)=database.read_with_revision("secrets",self.key()?)?;
        let plain=record.map(|record|{
            if record.payload.len()>STATE_BYTES{return Err("Protected SQLite state exceeds budget.".into());}
            self.0.sealer.unseal(&record.payload)
        }).transpose()?;
        Ok((revision,plain))
    }
}
impl StateStorage for SealedSqliteStore {
    fn host_snapshot_store(&self)->Option<&dyn super::HostSnapshotStore>{
        (self.key().ok()==Some("companion-host")).then_some(self)
    }
    fn read(&self)->Result<Option<Vec<u8>>,String>{
        let mut expected=self.0.revision.lock().map_err(|_|"Protected-store access failed.")?;
        let Some(database)=self.database(false)? else{*expected=Some(0);return Ok(None)};
        let(revision,plain)=self.read_record(database)?;*expected=Some(revision);Ok(plain)
    }
    fn write(&self,bytes:&[u8])->Result<(),String>{
        if bytes.is_empty() || bytes.len()>STATE_BYTES{return Err("Protected state exceeds budget.".into());}
        let mut expected=self.0.revision.lock().map_err(|_|"Protected-store access failed.")?;
        let database=self.database(true)?.ok_or("Protected namespace is unavailable.")?;
        // Check existing ciphertext before any key-creating seal/probe. Missing
        // or locked keys must never generate replacements over saved authority.
        let (revision,old)=self.read_record(database)?;
        if let Some(mut old)=old{old.zeroize();}
        if expected.is_some_and(|expected|expected!=revision){return Err("Protected-state revision conflict. Reload before saving.".into());}
        // No plaintext is bound into a SQLite statement, including failed writes.
        let encrypted=self.0.sealer.seal(bytes)?;
        if encrypted.len()>STATE_BYTES{return Err("Protected SQLite state exceeds budget.".into());}
        let next=database.commit(&[Change{domain:"secrets",key:self.key()?,expected_revision:revision,payload:Some(&encrypted)}])?[0];
        *expected=Some(next);Ok(())
    }
    fn available(&self)->Result<(),String>{
        if let Some(database)=self.database(false)? {
            let(_,plain)=self.read_record(database)?;
            if let Some(mut plain)=plain {plain.zeroize();return Ok(())}
        }
        // A status probe must not advance the revision of a previously loaded
        // credential. Only an explicit read replaces that owner's snapshot.
        self.0.sealer.available()
    }
    fn delete(&self)->Result<(),String>{
        let mut expected=self.0.revision.lock().map_err(|_|"Protected-store access failed.")?;
        let Some(database)=self.database(false)? else{*expected=Some(0);return Ok(())};
        // Explicit Forget can discard a corrupt opaque record. It still cannot
        // delete a newer credential over an earlier acknowledged revision.
        let(revision,record)=database.read_with_revision("secrets",self.key()?)?;
        if record.is_none(){*expected=Some(revision);return Ok(())}
        let next=database.commit(&[Change{domain:"secrets",key:self.key()?,expected_revision:expected.unwrap_or(revision),payload:None}])?[0];
        *expected=Some(next);Ok(())
    }
}
#[path="sqlite_host.rs"]
mod host;
#[cfg(test)]
#[path="sqlite_tests.rs"]
mod tests;

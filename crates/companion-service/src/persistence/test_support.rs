//! Portable authenticated encryption for integration fixtures, never production keys.
use super::*;
use std::sync::Arc;
struct Sealer;
impl NativeSealer for Sealer {
    fn available(&self)->Result<(),String>{Ok(())}
    fn seal(&self,bytes:&[u8])->Result<Vec<u8>,String>{let mut nonce=[0;12];getrandom::fill(&mut nonce).map_err(|_|"No randomness.")?;encrypt_with_key(bytes,&[5;32],nonce)}
    fn unseal(&self,bytes:&[u8])->Result<Vec<u8>,String>{decrypt_with_key(bytes,&[5;32])}
}
pub(crate) fn store(anchor:impl Into<PathBuf>)->SealedSqliteStore{SealedSqliteStore::with_sealer(anchor,Arc::new(Sealer))}

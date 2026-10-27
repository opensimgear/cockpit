//! Reusable cached metadata lookup; enrichment failure never blocks discovery.
use super::*;
use std::{collections::BTreeMap,path::Path,time::{Duration,Instant}};

const MAX_QUERIES:usize=512;
const BATCH_SIZE:usize=5;
const TOTAL_BUDGET:Duration=Duration::from_secs(12);

pub(crate) fn cached(root:&Path,query:&MetadataQuery)->Option<MetadataLookup> {
    let now=network::now();
    cache::read(root,query,now,true).or_else(||cache::read_source(root,query,now))
}

pub(crate) fn load(root:&Path,queries:&[MetadataQuery],cancelled:impl Fn()->bool+Sync)->BTreeMap<String,MetadataLookup> {
    load_with(root,queries,&cancelled,network::now,|batch,deadline| {
        network::request(root,&wikidata::query(batch),&cancelled,deadline)
    })
}

pub(crate) fn refresh(root:&Path,queries:&[MetadataQuery],cancelled:impl Fn()->bool+Sync)->BTreeMap<String,MetadataLookup> {
    load_using(root,queries,&cancelled,true,network::now,|batch,deadline| {
        network::request(root,&wikidata::query(batch),&cancelled,deadline)
    })
}

fn load_with<C,T,F>(root:&Path,queries:&[MetadataQuery],cancelled:&C,now:T,fetch:F)->BTreeMap<String,MetadataLookup>
where C:Fn()->bool+Sync,T:Fn()->u64,F:Fn(&[MetadataQuery],Instant)->Result<Vec<u8>,String> {
    load_using(root,queries,cancelled,false,now,fetch)
}
fn load_using<C,T,F>(root:&Path,queries:&[MetadataQuery],cancelled:&C,refresh:bool,now:T,fetch:F)->BTreeMap<String,MetadataLookup>
where C:Fn()->bool+Sync,T:Fn()->u64,F:Fn(&[MetadataQuery],Instant)->Result<Vec<u8>,String> {
    let deadline=Instant::now()+TOTAL_BUDGET;
    let mut results=BTreeMap::new();
    let mut missing=BTreeMap::<String,Vec<MetadataQuery>>::new();
    for query in queries.iter().take(MAX_QUERIES).filter(|query|query.valid()) {
        if cancelled()||Instant::now()>=deadline {return results;}
        if let Some(lookup)=(!refresh).then(||cache::read(root,query,now(),false)).flatten() {
            let _=cache::write(root,query,&lookup);
            results.insert(query.key.clone(),lookup);
        }
        else {missing.entry(cache::fingerprint(query)).or_default().push(query.clone());}
    }
    let groups=missing.into_values().collect::<Vec<_>>();
    for batch in groups.chunks(BATCH_SIZE) {
        if cancelled()||Instant::now()>=deadline {break;}
        let queries=batch.iter().filter_map(|group|group.first().cloned()).collect::<Vec<_>>();
        let parsed=fetch(&queries,deadline).and_then(|bytes| {
            if bytes.len()>network::MAX_RESPONSE {return Err("Wikidata metadata response exceeds its size limit.".into());}
            wikidata::parse(&queries,&bytes,now())
        });
        if cancelled() {break;}
        for group in batch {
            let query=&group[0];
            let lookup=parsed.as_ref().ok().and_then(|values|values.get(&query.key)).filter(|value|value.valid()&&!value.stale)
                .cloned().unwrap_or_else(||cache::read(root,query,now(),true)
                    .unwrap_or_else(||MetadataLookup::empty(LookupStatus::Unavailable,now())));
            for duplicate in group {
                if parsed.is_ok() {let _=cache::write(root,duplicate,&lookup);}
                results.insert(duplicate.key.clone(),lookup.clone());
            }
        }
    }
    results
}

#[cfg(test)]
#[path="provider_tests.rs"]
mod tests;

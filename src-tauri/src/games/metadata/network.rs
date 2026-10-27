//! One shared, paced Wikidata transport with a conservative rolling budget.
use super::cache;
use std::{collections::VecDeque,io::Read,path::Path,sync::{Mutex,OnceLock},time::{Duration,Instant,SystemTime,UNIX_EPOCH}};

pub(super) const MAX_RESPONSE:usize=2*1024*1024;
const MAX_REQUEST:Duration=Duration::from_secs(4);
const WINDOW:Duration=Duration::from_secs(60);
const RESERVED_BUDGET:Duration=Duration::from_secs(30);
const USER_AGENT:&str="OpenSimGear/0.1 (https://github.com/opensimgear/cockpit) Wikidata metadata";

pub(super) struct Reply {pub status:u16,pub retry_after:Option<String>,pub bytes:Vec<u8>}
#[derive(Default)]
struct Pace {reservations:VecDeque<(Instant,Duration)>,last:Option<Instant>,cooldown:u64,failures:u32}
impl Pace {
    fn delay(&mut self,instant:Instant,now:u64)->Duration {
        while self.reservations.front().is_some_and(|(at,_)|instant.saturating_duration_since(*at)>=WINDOW) {self.reservations.pop_front();}
        let mut wait=Duration::from_secs(self.cooldown.saturating_sub(now));
        if let Some(last)=self.last {wait=wait.max(Duration::from_secs(1).saturating_sub(instant.saturating_duration_since(last)));}
        let reserved=self.reservations.iter().map(|(_,duration)|*duration).sum::<Duration>();
        if reserved+MAX_REQUEST>RESERVED_BUDGET {
            if let Some((first,_))=self.reservations.front() {wait=wait.max(WINDOW.saturating_sub(instant.saturating_duration_since(*first)));}
        }
        wait
    }
    fn reserve(&mut self,instant:Instant) {self.last=Some(instant);self.reservations.push_back((instant,MAX_REQUEST));}
    fn failure(&mut self,now:u64,retry:Option<&str>)->u64 {
        self.failures=self.failures.saturating_add(1);
        let fallback=5u64.saturating_mul(1u64<<self.failures.saturating_sub(1).min(8)).min(3600);
        self.cooldown=now.saturating_add(retry_delay(retry,now).unwrap_or(fallback).max(5));
        self.cooldown
    }
}

pub(super) fn now()->u64 {SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()}

pub(super) fn request(root:&Path,query:&str,cancelled:&(impl Fn()->bool+Sync+?Sized),deadline:Instant)->Result<Vec<u8>,String> {
    static PACE:OnceLock<Mutex<Pace>>=OnceLock::new();
    let pace=PACE.get_or_init(||Mutex::new(Pace::default()));
    loop {
        if cancelled()||Instant::now()>=deadline {return Err("Wikidata metadata request was cancelled or exceeded its discovery budget.".into());}
        if let Ok(mut state)=pace.try_lock() {
            let instant=Instant::now();let current=now();
            state.cooldown=state.cooldown.max(cache::cooldown(root,current));
            let delay=state.delay(instant,current);
            if delay.is_zero() {
                let timeout=MAX_REQUEST.min(deadline.saturating_duration_since(instant));
                state.reserve(instant);
                // Keep the permit through transport, including response reading.
                let response=fetch(query,timeout);
                let result=complete(&mut state,response,now());
                if result.is_err() {cache::save_cooldown(root,state.cooldown);}
                return result;
            }
            if delay>=deadline.saturating_duration_since(instant) {return Err("Wikidata metadata is cooling down.".into());}
        }
        let remaining=deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {return Err("Wikidata metadata discovery budget exhausted.".into());}
        std::thread::sleep(remaining.min(Duration::from_millis(50)));
    }
}

fn complete(state:&mut Pace,response:Result<Reply,String>,current:u64)->Result<Vec<u8>,String> {
    match response {
        Ok(reply) if reply.status==200&&reply.bytes.len()<=MAX_RESPONSE=> {state.failures=0;Ok(reply.bytes)},
        Ok(reply)=> {
            let retry=matches!(reply.status,429|503).then_some(reply.retry_after.as_deref()).flatten();
            state.failure(current,retry);
            Err(format!("Wikidata metadata returned HTTP {} or exceeded its response limit.",reply.status))
        },
        Err(error)=> {state.failure(current,None);Err(error)},
    }
}

fn fetch(query:&str,timeout:Duration)->Result<Reply,String> {
    fetch_at("https://query.wikidata.org/sparql",query,timeout)
}

fn fetch_at(endpoint:&str,query:&str,timeout:Duration)->Result<Reply,String> {
    if timeout.is_zero()||query.len()>256*1024 {return Err("Invalid Wikidata metadata request.".into());}
    let agent=ureq::AgentBuilder::new().timeout(timeout).timeout_connect(timeout.min(Duration::from_secs(2)))
        .timeout_read(timeout).redirects(0).build();
    let response=match agent.get(endpoint).query("query",query).query("format","json")
        .set("Accept","application/sparql-results+json").set("User-Agent",USER_AGENT).call() {
        Ok(response)=>response,
        Err(ureq::Error::Status(_,response))=>response,
        Err(error)=>return Err(format!("Wikidata metadata request failed: {error}")),
    };
    let status=response.status();let retry_after=response.header("Retry-After").map(str::to_owned);
    if status!=200 {return Ok(Reply{status,retry_after,bytes:vec![]});}
    let bytes=read_bounded(response.into_reader())?;
    Ok(Reply{status,retry_after,bytes})
}

fn read_bounded(reader:impl Read)->Result<Vec<u8>,String> {
    let mut bytes=Vec::new();
    reader.take(MAX_RESPONSE as u64+1).read_to_end(&mut bytes).map_err(|error|error.to_string())?;
    if bytes.len()>MAX_RESPONSE {return Err("Wikidata metadata response exceeds its size limit.".into());}
    Ok(bytes)
}

fn retry_delay(value:Option<&str>,current:u64)->Option<u64> {
    let value=value?.trim();
    if let Ok(seconds)=value.parse::<u64>() {return Some(seconds);}
    let timestamp=httpdate::parse_http_date(value).ok()?.duration_since(UNIX_EPOCH).ok()?.as_secs();
    Some(timestamp.saturating_sub(current))
}

#[cfg(test)]
#[path="network_tests.rs"]
mod tests;

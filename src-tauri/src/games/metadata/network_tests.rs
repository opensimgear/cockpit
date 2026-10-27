use super::*;

fn server(status:&str,headers:&str,body:&str)->(String,std::thread::JoinHandle<String>) {
    use std::io::{Read,Write};
    let listener=std::net::TcpListener::bind("127.0.0.1:0").unwrap();let address=listener.local_addr().unwrap();
    let response=format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n{body}",body.len());
    let worker=std::thread::spawn(move|| {
        let (mut stream,_)=listener.accept().unwrap();stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let mut request=Vec::new();let mut chunk=[0;2048];
        while !request.windows(4).any(|value|value==b"\r\n\r\n") {
            let count=stream.read(&mut chunk).unwrap();assert!(count>0);request.extend_from_slice(&chunk[..count]);assert!(request.len()<32*1024);
        }
        stream.write_all(response.as_bytes()).unwrap();String::from_utf8(request).unwrap()
    });
    (format!("http://{address}/sparql"),worker)
}

#[test]
fn transport_identifies_cockpit_encodes_queries_and_preserves_throttling_headers() {
    let (url,worker)=server("200 OK","Content-Type: application/sparql-results+json\r\n","{\"results\":{\"bindings\":[]}}");
    let reply=fetch_at(&url,"SELECT ?game WHERE { VALUES ?title { \"Portal 2\"@en } }",Duration::from_secs(2)).unwrap();
    assert_eq!(reply.status,200);assert!(!reply.bytes.is_empty());
    let request=worker.join().unwrap();let lower=request.to_ascii_lowercase();
    assert!(request.starts_with("GET /sparql?"));assert!(request.contains("format=json"));
    assert!(lower.contains("user-agent: opensimgear/0.1 (https://github.com/opensimgear/cockpit) wikidata metadata"));
    assert!(lower.contains("accept: application/sparql-results+json"));assert!(!request.contains("\"Portal 2\""));
    let (url,worker)=server("429 Too Many Requests","Retry-After: 120\r\n","rate limited");
    let reply=fetch_at(&url,"SELECT ?game {}",Duration::from_secs(2)).unwrap();worker.join().unwrap();
    assert_eq!(reply.status,429);assert_eq!(reply.retry_after.as_deref(),Some("120"));assert!(reply.bytes.is_empty());
    assert!(fetch_at(&url,"query",Duration::ZERO).is_err());
    assert!(fetch_at(&url,&"x".repeat(256*1024+1),Duration::from_secs(1)).is_err());
}

#[test]
fn pacing_reserves_at_most_twenty_eight_seconds_per_rolling_minute() {
    let mut state=Pace::default();let start=Instant::now();
    for index in 0..7 {
        let at=start+Duration::from_secs(index);
        assert!(state.delay(at,100+index).is_zero());state.reserve(at);
    }
    assert_eq!(state.delay(start+Duration::from_secs(7),107),Duration::from_secs(53));
    assert!(state.delay(start+Duration::from_secs(60),160).is_zero());
}

#[test]
fn pacing_enforces_one_second_between_starts() {
    let mut state=Pace::default();let start=Instant::now();state.reserve(start);
    assert_eq!(state.delay(start+Duration::from_millis(250),100),Duration::from_millis(750));
}

#[test]
fn retry_after_accepts_seconds_and_http_dates_and_backoff_increases() {
    assert_eq!(retry_delay(Some("120"),100),Some(120));
    assert_eq!(retry_delay(Some("Thu, 01 Jan 1970 00:03:20 GMT"),100),Some(100));
    assert_eq!(retry_delay(Some("invalid"),100),None);
    let mut state=Pace::default();assert_eq!(state.failure(100,None),105);
    assert_eq!(state.failure(100,None),110);
    assert_eq!(state.failure(100,Some("120")),220);
    assert_eq!(state.delay(Instant::now(),200),Duration::from_secs(20));
    assert_eq!(state.failure(100,Some("259200")),259300);
    assert_eq!(state.failure(100,Some("Sun, 04 Jan 1970 00:00:00 GMT")),259200);
    assert_eq!(state.failure(100,Some("18446744073709551615")),u64::MAX);
}

#[test]
fn status_failures_and_transport_errors_cool_down_but_success_resets_backoff() {
    let mut state=Pace::default();
    assert!(complete(&mut state,Ok(Reply{status:429,retry_after:Some("80".into()),bytes:vec![]}),100).is_err());
    assert_eq!(state.cooldown,180);
    assert!(complete(&mut state,Ok(Reply{status:503,retry_after:Some("Thu, 01 Jan 1970 00:05:00 GMT".into()),bytes:vec![]}),200).is_err());
    assert_eq!(state.cooldown,300);
    assert!(complete(&mut state,Err("offline".into()),300).is_err());assert_eq!(state.cooldown,320);
    assert_eq!(complete(&mut state,Ok(Reply{status:200,retry_after:None,bytes:b"{}".to_vec()}),400).unwrap(),b"{}");
    assert_eq!(state.failures,0);
}

#[test]
fn bounded_response_accepts_limit_and_rejects_extra_byte() {
    assert_eq!(read_bounded(&vec![0;MAX_RESPONSE][..]).unwrap().len(),MAX_RESPONSE);
    assert!(read_bounded(&vec![0;MAX_RESPONSE+1][..]).is_err());
    let mut state=Pace::default();
    assert!(complete(&mut state,Ok(Reply{status:200,retry_after:None,bytes:vec![0;MAX_RESPONSE+1]}),100).is_err());
}

#[test]
fn cancellation_expired_deadline_and_persisted_cooldown_never_use_transport() {
    let root=tempfile::tempdir().unwrap();
    assert!(request(root.path(),"unused",&||true,Instant::now()+Duration::from_secs(1)).is_err());
    assert!(request(root.path(),"unused",&||false,Instant::now()).is_err());
    cache::save_cooldown(root.path(),now()+60);
    assert!(request(root.path(),"unused",&||false,Instant::now()+Duration::from_millis(100)).is_err());
}

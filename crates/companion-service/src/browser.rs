//! Browser assets share the opt-in TLS companion listener. No filesystem or operator API.
use std::io::{Read,Write};
use tungstenite::handshake::{machine::TryParse, server::Request};
pub struct BrowserAsset { pub path: &'static str, pub mime: &'static str, pub bytes: &'static [u8] }
pub(crate) fn read_header<S:Read>(stream:&mut S)->Result<(Request,Vec<u8>),String>{
    let mut bytes=Vec::new();
    while !bytes.ends_with(b"\r\n\r\n"){
        if bytes.len()>=8192{return Err("HTTP header exceeds budget".into());}
        let mut byte=[0];stream.read_exact(&mut byte).map_err(|e|e.to_string())?;bytes.push(byte[0]);
    }
    let request=Request::try_parse(&bytes).map_err(|e|e.to_string())?.ok_or("Invalid HTTP request")?.1;
    Ok((request,bytes))
}
pub(crate) fn allowed(request:&Request,origin:&str)->bool{
    request.headers().get("origin").and_then(|v|v.to_str().ok())==Some(origin)
        && request.headers().get("host").and_then(|v|v.to_str().ok())==origin.strip_prefix("https://")
}
pub(crate) fn serve_http<S:Write>(stream:&mut S,request:&Request,origin:&str,assets:&[BrowserAsset])->Result<(),String>{
    let host=request.headers().get("host").and_then(|v|v.to_str().ok())==origin.strip_prefix("https://");
    let (status,mime,body)=if !host || request.method()!="GET" {
        ("403 Forbidden","text/plain",b"Forbidden".as_slice())
    }else if let Some(asset)=assets.iter().find(|a|a.path==request.uri().path()){
        ("200 OK",asset.mime,asset.bytes)
    }else{("404 Not Found","text/plain",b"Not found".as_slice())};
    write!(stream,"HTTP/1.1 {status}\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\nContent-Security-Policy: default-src 'self'; script-src 'self'; worker-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self'; object-src 'none'; frame-ancestors 'none'; base-uri 'none'\r\n\r\n",body.len()).map_err(|e|e.to_string())?;
    stream.write_all(body).map_err(|e|e.to_string())
}
pub(crate) struct Replay<S>{pub prefix:std::io::Cursor<Vec<u8>>,pub stream:S}
impl<S:Read> Read for Replay<S>{fn read(&mut self,b:&mut[u8])->std::io::Result<usize>{if self.prefix.position()<self.prefix.get_ref().len() as u64{self.prefix.read(b)}else{self.stream.read(b)}}}
impl<S:Write> Write for Replay<S>{fn write(&mut self,b:&[u8])->std::io::Result<usize>{self.stream.write(b)}fn flush(&mut self)->std::io::Result<()>{self.stream.flush()}}

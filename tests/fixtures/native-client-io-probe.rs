use osg_companion_service::client::ClientRuntime;
use serde_json::{json,Value};
use std::io::{self,BufRead,Write};
fn main() {
 let directory=std::env::args().nth(1).expect("isolated data directory");
 let client=ClientRuntime::open(directory.into()).expect("native client initialization");
 for line in io::stdin().lock().lines() {
  let line=line.expect("stdin");
  let response=match serde_json::from_str::<Value>(&line).map_err(|error|error.to_string()).and_then(|request|client.command(request)) {
   Ok(state)=>json!({"ok":true,"state":state}),Err(error)=>json!({"ok":false,"error":error}),
  };
  println!("{}",response);io::stdout().flush().expect("stdout");
 }
}

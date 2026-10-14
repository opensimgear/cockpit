use osg_mapping::trace::{Trace, TRACE_LIMIT};
use std::io::Read;
fn main() {
    let result = (|| -> Result<(), String> {
        let path = std::env::args()
            .nth(1)
            .ok_or("Usage: cargo run --example replay -- trace.json")?;
        let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
        let mut bytes = Vec::new();
        file.take(TRACE_LIMIT as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        let trace = Trace::parse(&bytes)?;
        for record in trace.replay()? {
            println!(
                "{}",
                serde_json::to_string(&record).map_err(|e| e.to_string())?
            );
        }
        Ok(())
    })();
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

use std::io::Read;
fn main() -> std::process::ExitCode {
    if let Some(code) = osg_logic::helper_main() {
        return std::process::ExitCode::from(code as u8);
    }
    if std::env::args().nth(1).as_deref() != Some("replay") {
        eprintln!("Usage: osg-logic replay <trace.json>");
        return std::process::ExitCode::from(2);
    }
    let result = (|| -> Result<(), String> {
        let path = std::env::args().nth(2).ok_or("Trace path required.")?;
        let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
        let mut bytes = Vec::new();
        file.take(osg_mapping::trace::TRACE_LIMIT as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        let trace = osg_mapping::trace::Trace::parse(&bytes)?;
        let reports = osg_logic::replay::replay(
            &trace,
            &std::env::current_exe().map_err(|e| e.to_string())?,
        )?;
        println!(
            "{}",
            serde_json::json!({"reports":reports.len(),"sha256":osg_mapping::trace::digest(&serde_json::to_vec(&reports).unwrap()),"profileSha256":osg_mapping::trace::fingerprint(&trace.profile)?})
        );
        Ok(())
    })();
    if let Err(error) = result {
        eprintln!("{error}");
        return std::process::ExitCode::FAILURE;
    }
    std::process::ExitCode::SUCCESS
}

//! Resolve exported DCS aircraft names only through the installed alias table.
use super::*;

fn module_name(value: &str) -> bool {
    dcs_bios::safe_module_name(value)
        && !matches!(value, "CommonData" | "NS430" | "MetadataStart" | "MetadataEnd" | "MetaDataStart" | "MetaDataEnd")
}
fn aliases(path: &Path) -> Result<BTreeMap<String, Vec<String>>, String> {
    let document = dcs_bios::read_json(&path.join("AircraftAliases.json"), dcs_bios::MAX_DOCUMENT)?;
    let object = document.as_object().filter(|o| o.len() <= 1000).ok_or(CATALOGUE_ERROR)?;
    object.iter().map(|(name, values)| {
        if name.len() > 63 || name.chars().any(char::is_control) { return Err(CATALOGUE_ERROR.into()); }
        let values = values.as_array().filter(|v| v.len() <= 32).ok_or(CATALOGUE_ERROR)?;
        let modules = values.iter().map(|value| text(value, 63, false).map(str::to_owned).ok_or_else(|| CATALOGUE_ERROR.to_owned())).collect::<Result<Vec<_>,_>>()?;
        Ok((name.clone(), modules))
    }).collect()
}
pub(crate) fn resolve_aircraft_module(path: &Path, aircraft: &str) -> Result<String, String> {
    if aircraft.is_empty() || aircraft.len() > 63 || aircraft.chars().any(char::is_control) {
        return Err("Choose a known DCS aircraft.".into());
    }
    let aliases = aliases(path)?;
    resolve(path, &aliases, aircraft)
}
fn resolve(path: &Path, aliases: &BTreeMap<String, Vec<String>>, aircraft: &str) -> Result<String, String> {
    let eligible = |module: &str| module_name(module)
        && aliases.values().any(|values| values.iter().any(|value| value == module))
        && catalogue_file(&path.join(format!("{module}.json"))).ok().flatten().is_some();
    // A self-named aircraft is already the canonical module. Alias entries that
    // include multiple independent aircraft modules remain ambiguous.
    if eligible(aircraft) && aliases.get(aircraft).is_none_or(|modules| modules.iter().any(|module|module == aircraft)) { return Ok(aircraft.into()); }
    let modules = aliases.get(aircraft).ok_or_else(|| format!("DCS aircraft {aircraft} has no installed DCS-BIOS alias."))?;
    let candidates = modules.iter().filter(|module| eligible(module)).cloned().collect::<BTreeSet<_>>();
    if candidates.len() != 1 {
        return Err(format!("DCS aircraft {aircraft} does not resolve to one installed aircraft module."));
    }
    Ok(candidates.into_iter().next().unwrap())
}
pub(crate) fn aircraft_aliases(path: &Path, module: &str) -> Result<Vec<String>, String> {
    if !module_name(module) { return Err("Choose an actual DCS aircraft module.".into()); }
    let aliases = aliases(path)?;
    let mut result = aliases.iter().filter_map(|(name, modules)| {
        (!name.is_empty() && modules.iter().any(|value| value == module)
            && resolve(path, &aliases, name).as_deref() == Ok(module)).then_some(name.clone())
    }).collect::<Vec<_>>();
    result.sort();
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn aliases_resolve_one_installed_module_and_reject_unknown_ambiguous_or_paths() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("AircraftAliases.json"), json!({"A-10C":["A-10C","CommonData"],"A-10C_2":["A-10C","CommonData"],"F-16C_50":["F-16C_50","CommonData"],"F-14B":["F-14","CommonData","NS430"],"F-86F":["F-86F Sabre","CommonData","NS430"],"Ambiguous":["A-10C","F-16C_50"],"../outside":["../outside"]}).to_string()).unwrap();
        for module in ["A-10C", "F-16C_50", "F-14", "F-86F Sabre", "NS430"] { fs::write(dir.path().join(format!("{module}.json")), "{}").unwrap(); }
        assert_eq!(resolve_aircraft_module(dir.path(), "A-10C_2").unwrap(), "A-10C");
        assert_eq!(resolve_aircraft_module(dir.path(), "F-16C_50").unwrap(), "F-16C_50");
        assert_eq!(resolve_aircraft_module(dir.path(), "F-14B").unwrap(), "F-14");
        assert_eq!(resolve_aircraft_module(dir.path(), "F-14").unwrap(), "F-14");
        assert_eq!(resolve_aircraft_module(dir.path(), "F-86F").unwrap(), "F-86F Sabre");
        for bad in ["Missing", "Ambiguous", "../outside", "CommonData"] { assert!(resolve_aircraft_module(dir.path(), bad).is_err(), "{bad}"); }
        assert_eq!(aircraft_aliases(dir.path(), "A-10C").unwrap(), vec!["A-10C", "A-10C_2"]);
    }
}

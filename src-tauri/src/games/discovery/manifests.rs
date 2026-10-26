// A small bounded KeyValues reader accepts current and legacy Steam manifests.
// It preserves Windows backslashes and Unicode, and rejects incomplete objects.
#[derive(Debug, PartialEq)]
pub(crate) enum Vdf {
    Text(String),
    Object(Vec<(String, Vdf)>),
}
impl Vdf {
    pub(crate) fn get(&self, name: &str) -> Option<&Vdf> {
        match self {
            Self::Object(entries) => entries
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(name))
                .map(|(_, v)| v),
            _ => None,
        }
    }
    pub(crate) fn text(&self) -> Option<&str> {
        match self {
            Self::Text(s) => Some(s),
            _ => None,
        }
    }
}
fn parse_vdf(input: &str) -> Result<Vdf, String> {
    if input.len() > 1_048_576 {
        return Err("Steam manifest is too large.".into());
    }
    let mut chars = input.chars().peekable();
    let mut tokens = Vec::new();
    while let Some(c) = chars.next() {
        if c.is_whitespace() || c == '\u{feff}' {
            continue;
        }
        if c == '/' && chars.peek() == Some(&'/') {
            for c in chars.by_ref() {
                if c == '\n' {
                    break;
                }
            }
            continue;
        }
        if c == '{' || c == '}' {
            tokens.push(c.to_string());
            continue;
        }
        let mut word = String::new();
        if c == '"' {
            let mut closed = false;
            while let Some(c) = chars.next() {
                if c == '"' {
                    closed = true;
                    break;
                }
                if c == '\\' && matches!(chars.peek(), Some('\\' | '"')) {
                    word.push(chars.next().unwrap());
                } else {
                    word.push(c);
                }
            }
            if !closed {
                return Err("Steam manifest has an incomplete string.".into());
            }
        } else {
            word.push(c);
            while chars
                .peek()
                .is_some_and(|c| !c.is_whitespace() && *c != '{' && *c != '}')
            {
                word.push(chars.next().unwrap());
            }
        }
        // Prefixing scalar tokens means quoted literal braces stay scalars.
        tokens.push(format!("s{word}"));
    }
    fn object(tokens: &[String], cursor: &mut usize, depth: usize) -> Result<Vdf, String> {
        if depth > 16 {
            return Err("Steam manifest nesting is too deep.".into());
        }
        let mut entries = Vec::new();
        while *cursor < tokens.len() {
            if tokens[*cursor] == "}" {
                if depth == 0 {
                    return Err("Steam manifest has an unexpected brace.".into());
                }
                *cursor += 1;
                return Ok(Vdf::Object(entries));
            }
            let key = tokens[*cursor]
                .strip_prefix('s')
                .ok_or("Steam manifest has an invalid key.")?
                .to_owned();
            *cursor += 1;
            let value = tokens
                .get(*cursor)
                .ok_or("Steam manifest has a missing value.")?;
            *cursor += 1;
            let value = if value == "{" {
                object(tokens, cursor, depth + 1)?
            } else {
                Vdf::Text(
                    value
                        .strip_prefix('s')
                        .ok_or("Steam manifest has an invalid value.")?
                        .into(),
                )
            };
            entries.push((key, value));
        }
        if depth != 0 {
            return Err("Steam manifest has an incomplete object.".into());
        }
        Ok(Vdf::Object(entries))
    }
    object(&tokens, &mut 0, 0)
}
pub(crate) fn read_vdf(path: &Path) -> Option<Vdf> {
    let mut input = String::new();
    fs::File::open(path)
        .ok()?
        .take(1_048_577)
        .read_to_string(&mut input)
        .ok()?;
    parse_vdf(&input).ok()
}
pub(crate) fn library_paths(root: &Path) -> Vec<PathBuf> {
    let mut paths = vec![root.to_path_buf()];
    let manifest = read_vdf(&root.join("steamapps/libraryfolders.vdf"))
        .or_else(|| read_vdf(&root.join("config/libraryfolders.vdf")));
    if let Some(Vdf::Object(entries)) = manifest.as_ref().and_then(|m| m.get("libraryfolders")) {
        for (key, value) in entries {
            if key.parse::<u32>().is_err() {
                continue;
            }
            if let Some(path) = value
                .text()
                .or_else(|| value.get("path").and_then(Vdf::text))
            {
                let path = PathBuf::from(path);
                if path.is_absolute() {
                    add_path(&mut paths, path);
                }
            }
        }
    }
    paths
}
fn steam_installation(game: &RecognizedGame, library: &Path) -> Option<PathBuf> {
    let manifest = read_vdf(&library.join(format!("steamapps/appmanifest_{}.acf", game.steam_id)))?;
    let state = manifest.get("AppState")?;
    if state.get("appid")?.text()?.parse::<u32>().ok()? != game.steam_id {
        return None;
    }
    let name = state.get("installdir")?.text()?;
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\', ':', '\0']) {
        return None;
    }
    Some(library.join("steamapps/common").join(name))
}
fn discover(roots: &Roots, manual: &BTreeMap<String, String>) -> Vec<Value> {
    let mut steam_installs = Vec::new();
    let steam_executable = roots
        .steam
        .iter()
        .map(|p| p.join("steam.exe"))
        .find(|p| p.is_file());
    for steam in &roots.steam {
        for library in library_paths(steam) {
            for game in GAMES {
                if let Some(root) = steam_installation(game, &library) {
                    if let Some(install) = installation(game, &root) {
                        steam_installs.push((game.id, install));
                    }
                }
            }
        }
    }
    GAMES.iter().map(|game| {
        let selected = manual.get(game.id).filter(|s| !s.trim().is_empty());
        let mut source = "not-found";
        let found = if let Some(selected) = selected {
            source = "manual";
            installation(game, Path::new(selected))
        } else if let Some((_, install)) = steam_installs.iter().find(|(id,_)| *id == game.id) {
            source = "steam";
            Some(install.clone())
        } else if game.id == "dcs" {
            let install = roots.dcs.iter().find_map(|p|installation(game,p));
            if install.is_some() { source = "standalone"; }
            install
        } else if game.provider == "x-plane" {
            let install=crate::xplane_installation::roots().iter().find_map(|p|installation(game,p));
            if install.is_some() {source="standalone";}
            install
        } else { None };
        let matches_steam = found.as_ref().is_some_and(|(_, _, root)| {
            steam_installs.iter().any(|(id, (_, _, steam_root))| {
                *id == game.id && paths_match(&root.to_string_lossy(), &steam_root.to_string_lossy())
            })
        });
        let steam = game.provider != "x-plane" && (source == "steam" || matches_steam || game.id.starts_with("assetto-corsa") && found.is_some());
        // X-Plane supports direct launch, including a selected secondary copy.
        // Steam -applaunch may launch another installation, so monitor/launch the exact executable.
        let mut installations: Vec<Value> = steam_installs.iter().filter(|(id,_)|*id==game.id).map(|(_, (exe,paths,root))|json!({"executable":exe,"executables":paths,"installPath":root,"source":"steam"})).collect();
        let mut installation_paths=BTreeSet::new();
        installations.retain(|i|installation_paths.insert(normalized_path(i["executable"].as_str().unwrap())));
        let standalone = if game.provider == "x-plane" { crate::xplane_installation::roots() } else if game.id == "dcs" { roots.dcs.clone() } else { vec![] };
        for root in standalone {
            if let Some((exe,paths,root)) = installation(game,&root) {
                if !installations.iter().any(|item|item["executable"] == json!(exe)) {
                    installations.push(json!({"executable":exe,"executables":paths,"installPath":root,"source":"standalone"}));
                }
            }
        }
        if let Some((exe,paths,root)) = &found {
            if !installations.iter().any(|item|item["executable"] == json!(exe)) {
                installations.push(json!({"executable":exe,"executables":paths,"installPath":root,"source":source}));
            }
        }
        let executable = found.as_ref().and_then(|(exe,_,_)|path_string(exe));
        let install_path = found.as_ref().and_then(|(_,_,root)|path_string(root));
        let process_executables: Vec<_> = found.as_ref().map(|(_,paths,_)|paths.iter().filter_map(|p|path_string(p)).collect()).unwrap_or_default();
        let launch_supported = roots.launch_supported && found.is_some() && (!steam || steam_executable.is_some());
        let reason = if !roots.launch_supported { Some("Game launch and process monitoring require Windows.") }
            else if steam && steam_executable.is_none() { Some("Install Steam to launch this game.") }
            else { None };
        json!({"id":game.id,"name":game.name,"provider":game.provider,"integrationKey":game.id,"integrationCompatible":found.is_some(),"integrationAvailable":true,
            "installed":found.is_some(),"executable":executable,"installPath":install_path,
            "source":source,"manualPath":selected,"installations":installations,"launchSupported":launch_supported,"processMonitoringSupported":roots.launch_supported,
            "launchUnavailableReason":reason,"steamAppId":if steam {Some(game.steam_id)} else {None},
            "steamExecutable":if steam {steam_executable.as_ref().and_then(|p|path_string(p))} else {None},
            "processExecutables":process_executables})
    }).filter(|game|game["installed"] == true).collect()
}

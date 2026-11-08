use super::*;

pub(super) fn inspect_handoff(
    session: &mut Session<Box<dyn Adapter>>,
    request: &ReadRequest,
    job: &str,
    cancel: &AtomicBool,
    observe: &Observe,
    paths: &[(String, String)],
) -> Result<(State, Plan), String> {
    let check = || -> Result<(), String> {
        live_read(request.deadline, cancel)?;
        if request.abandoned.load(Ordering::Acquire) || request.input.job != job {
            return Err(
                "The native visibility preparation ticket no longer owns this worker.".into(),
            );
        }
        Ok(())
    };
    check()?;
    let input = &request.input;
    input.preferences.validate()?;
    if !input.preferences.enabled {
        return Err("Enable optional visibility before preparation.".into());
    }
    let ids = input
        .sources
        .iter()
        .map(|s| s.id.clone())
        .collect::<Vec<_>>();
    let outgoing = session.sources.clone();
    let outgoing_ids = outgoing.iter().map(|s| s.id.clone()).collect::<Vec<_>>();
    session.inspect(|adapter, record| {
        check()?;
        if serde_json::to_vec(record).map_err(|e| e.to_string())?
            != serde_json::to_vec(&input.record).map_err(|e| e.to_string())?
        {
            return Err(
                "The outgoing native visibility journal no longer matches the requested session."
                    .into(),
            );
        }
        let steps = record.validate()?;
        if record.clean
            || record.restoring
            || record.pending.is_some()
            || record.position + 1 != steps.len()
        {
            return Err(
                "Only a fully applied native visibility session can prepare handoff.".into(),
            );
        }
        session::validate_sources(&input.sources, &observe(&ids)?, false)?;
        session::validate_sources(&outgoing, &observe(&outgoing_ids)?, false)?;
        let actual = adapter.read()?;
        check()?;
        if actual != record.plan.after {
            return Err("The outgoing native driver policy changed; preserve it.".into());
        }
        let image = adapter.executable(&input.capture)?;
        check()?;
        // The live session's own executable and instances remain immutable.
        if image != record.plan.capture {
            return Err("The capture executable changed during native handoff.".into());
        }
        for (path, image) in paths {
            if adapter.executable(path)? != *image {
                return Err("An outgoing visibility executable changed.".into());
            }
            check()?;
        }
        if outgoing.len() != record.plan.selections.len() {
            return Err("The outgoing native selections changed.".into());
        }
        for (source, selection) in outgoing.iter().zip(&record.plan.selections) {
            let instance = adapter.instance(&source.interface)?;
            check()?;
            if instance != selection.instance {
                return Err("An outgoing hidden controller instance changed.".into());
            }
        }
        let mut selections = Vec::new();
        for source in &input.sources {
            let policy = match input.preferences.devices.get(&source.id) {
                Some(Intent::HideAll) => super::super::policy::Policy::HideAll,
                Some(Intent::HideFromProcess { executable }) => {
                    let image = adapter.executable(executable)?;
                    check()?;
                    super::super::policy::Policy::HideFromProcess { executable: image }
                }
                _ => return Err("Unsupported captured policy in native visibility handoff.".into()),
            };
            let instance = adapter.instance(&source.interface)?;
            check()?;
            selections.push(Selection { instance, policy });
        }
        let policy = Plan::build(record.plan.before.clone(), selections, image)?;
        session::validate_sources(&input.sources, &observe(&ids)?, false)?;
        session::validate_sources(&outgoing, &observe(&outgoing_ids)?, false)?;
        if adapter.read()? != actual {
            return Err("The native driver policy changed during handoff preparation.".into());
        }
        check()?;
        Ok((actual, policy))
    })
}

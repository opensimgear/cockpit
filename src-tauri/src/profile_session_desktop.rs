//! Desktop output belongs to the captured ProfileSession. The frontend cannot
//! send live effects; only the native typed reducer feeds this adapter.
use crate::{profile_desktop_settings as settings, profile_session::RunOwner};
use osg_desktop_output::{ProcessIdentity, Scope};
use osg_mapping::{engine::{typed::{Effect,Phase},Counter,RunToken},program::Program};
use serde_json::Value;
use std::sync::{Mutex, OnceLock};

static RECENT_APPLICATION: OnceLock<Mutex<Option<ProcessIdentity>>> = OnceLock::new();
pub(crate) fn observe_foreground() {
    if let Ok(identity) = foreground() {
        if std::env::current_exe().ok().is_some_and(|path| path.to_string_lossy().eq_ignore_ascii_case(&identity.executable)) { return; }
        *RECENT_APPLICATION.get_or_init(|| Mutex::new(None)).lock().unwrap_or_else(|p|p.into_inner()) = Some(identity);
    }
}
pub(crate) fn recent_application() -> Result<ProcessIdentity, String> {
    RECENT_APPLICATION.get_or_init(|| Mutex::new(None)).lock().unwrap_or_else(|p|p.into_inner()).clone()
        .ok_or_else(|| "Focus the application you want to control, then return to Cockpit and choose it.".into())
}

pub(crate) fn capabilities(backend: &mut osg_profiles::compiler_models::BackendCapabilities, native: bool) {
    let preferences = if native { settings::Settings::native() } else { settings::Settings::open(None) };
    backend.desktop_available = native && cfg!(windows);
    backend.desktop_qualified = native && settings::qualified();
    backend.desktop_enabled = native && preferences.error.is_none() && preferences.value.enabled;
}
pub(crate) fn identity(process: &crate::game_discovery::ProcessIdentity) -> ProcessIdentity {
    ProcessIdentity { executable: process.path.clone(), pid: process.pid, created: process.epoch.to_string() }
}
pub(crate) fn selected_game_scope(process:Option<crate::game_discovery::ProcessIdentity>)->Result<Scope,String>{
    let process=process.ok_or("The selected game has no verified running process for keyboard and mouse scope. Launch the game or wait for its native process before applying.")?;
    crate::game_discovery::ProcessIdentity::parse(&process.value())?;
    Ok(Scope::Application{identity:identity(&process)})
}
pub(crate) fn foreground() -> Result<ProcessIdentity, String> {
    #[cfg(windows)] { osg_desktop_output::windows::foreground_identity() }
    #[cfg(not(windows))] { Err("Native keyboard and mouse output is currently supported on Windows only.".into()) }
}
pub(crate) fn scope(game: Option<&crate::games::PreparedGame>) -> Result<Scope, String> {
    let preferences = settings::Settings::native();
    if let Some(error) = preferences.error { return Err(error); }
    match preferences.value.scope {
        settings::Scope::DesktopWide => Ok(Scope::DesktopWide),
        settings::Scope::Application => {
            if let Some(game) = game { return Ok(Scope::Application { identity: identity(&game.proof.process) }); }
            let captured = preferences.value.application.ok_or("Choose an application or explicitly select desktop-wide scope before applying keyboard and mouse output.")?;
            let process = crate::game_discovery::ProcessIdentity::parse(&captured)?;
            Ok(Scope::Application { identity: identity(&process) })
        }
    }
}
pub(crate) fn journal_name(owner: &RunOwner) -> Result<String, String> {
    if uuid::Uuid::parse_str(&owner.session).is_err() || owner.generation == 0 { return Err("Invalid native desktop run identity.".into()); }
    Ok(format!("profile-desktop-{}-{}.json", owner.session, owner.generation))
}
/// Thin installation/game adapter around the shared native mapping host.
pub(crate) struct Desktop(osg_mapping_host::desktop::Desktop);
impl Desktop {
    pub(crate) fn acquire(owner:&RunOwner,program:&Program,scope:Option<Scope>)->Result<Self,String>{
        let preferences=settings::Settings::native();
        if let Some(error)=preferences.error{return Err(error);}
        let run=RunToken{session:owner.session.clone(),generation:Counter(owner.generation)};
        osg_mapping_host::desktop::Desktop::acquire(run,program,preferences.value.enabled,settings::qualified(),
            crate::config_paths::config_path().with_file_name(journal_name(owner)?),scope).map(Self)
    }
    pub(crate) fn start(&mut self,reducer:&Phase,game:Option<&crate::games::PreparedGame>,captured:Option<Scope>)->Result<(),String>{
        if let Some(captured)=captured { self.0.bind_scope_before_start(captured)?; }
        let resolved=if self.0.scope().is_none(){Some(scope(game)?)}else{None};
        self.0.start(reducer,resolved)
    }
    pub(crate) fn poll(&mut self,effects:&[Effect],reducer:&Phase)->Result<Vec<String>,String>{self.0.poll(effects,reducer)}
    pub(crate) fn keep_alive(&mut self)->Result<(),String>{self.0.keep_alive()}
    pub(crate) fn stop(&mut self)->Result<(),String>{self.0.stop()}
    pub(crate) fn status(&self)->Value{self.0.status()}
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn selected_game_without_a_native_process_cannot_fall_back_to_a_local_application(){
        let unrelated=crate::game_discovery::ProcessIdentity{pid:99,path:"C:\\Fixture\\Other.exe".into(),epoch:88};
        assert!(selected_game_scope(None).unwrap_err().contains("selected game"));
        let game=crate::game_discovery::ProcessIdentity{pid:42,path:"C:\\Fixture\\Game.exe".into(),epoch:10};
        let scope=selected_game_scope(Some(game.clone())).unwrap();assert_eq!(scope,Scope::Application{identity:identity(&game)});
        assert_ne!(scope,Scope::Application{identity:identity(&unrelated)});
    }
}

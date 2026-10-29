//! Desktop output belongs to the captured ProfileSession. The frontend cannot
//! send live effects; only the native typed reducer feeds this adapter.
use crate::{profile_desktop_settings as settings, profile_session::RunOwner};
use osg_desktop_output::{ProcessIdentity, Scope};
use osg_mapping::{engine::{typed::{Frame,Phase},Counter,RunToken},program::Program};
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
/// The outer game option records an explicit context; its inner value is the
/// observed process. A missing process never falls back to another application.
pub(crate) fn prepare_scope(
    declared: Option<&osg_profiles::DesktopScope>, launch: bool,
    game: Option<Option<crate::game_discovery::ProcessIdentity>>, preferences: &settings::Preferences,
) -> Result<Option<Scope>, String> {
    use osg_profiles::DesktopScope;
    let application = |captured: &Value| -> Result<Option<Scope>, String> {
        let process = crate::game_discovery::ProcessIdentity::parse(captured)?;
        Ok(Some(Scope::Application { identity: identity(&process) }))
    };
    match declared {
        Some(DesktopScope::DesktopWide) => Ok(Some(Scope::DesktopWide)),
        Some(DesktopScope::Application { application_reference: Some(reference) }) => {
            let captured = preferences.applications.get(reference).ok_or("This profile's selected application is unavailable on this computer. Capture the application in its keyboard and mouse block, then save the profile.")?;
            application(captured)
        }
        Some(DesktopScope::Application { application_reference: None }) => Err("Choose an application in this profile's keyboard and mouse block, then save the profile.".into()),
        Some(DesktopScope::SelectedGame) => {
            if launch { return Ok(None); }
            let process = game.ok_or("Choose a game context for this profile's keyboard and mouse block, or select Global scope.")?;
            if process.is_none() { Ok(None) } else { selected_game_scope(process).map(Some) }
        }
        None => {
            // Older profiles retain an application-only default. A legacy
            // machine-wide desktop scope cannot silently broaden their output.
            if let Some(process) = game {
                if launch || process.is_none() { Ok(None) } else { selected_game_scope(process).map(Some) }
            } else {
                application(preferences.application.as_ref().ok_or("Choose an application or game context in this profile's keyboard and mouse block before applying.")?)
            }
        }
    }
}
pub(crate) fn journal_name(owner: &RunOwner) -> Result<String, String> {
    if uuid::Uuid::parse_str(&owner.session).is_err() || owner.generation == 0 { return Err("Invalid native desktop run identity.".into()); }
    Ok(format!("profile-desktop-{}-{}.json", owner.session, owner.generation))
}
/// Thin installation/game adapter around the shared native mapping host.
pub(crate) struct Desktop {
    inner: Option<osg_mapping_host::desktop::Desktop>,
    owner: RunOwner,
    program: Program,
    scope: Option<Scope>,
    started: bool,
}
impl Desktop {
    pub(crate) fn acquire(owner:&RunOwner,program:&Program,scope:Option<Scope>)->Result<Self,String>{
        let preferences=settings::Settings::native();
        if let Some(error)=preferences.error{return Err(error);}
        let run=RunToken{session:owner.session.clone(),generation:Counter(owner.generation)};
        let inner=osg_mapping_host::desktop::Desktop::acquire(run,program,preferences.value.enabled,settings::qualified(),
            crate::config_paths::config_path().with_file_name(journal_name(owner)?),scope.clone())?;
        Ok(Self { inner:Some(inner), owner:owner.clone(), program:program.clone(), scope, started:false })
    }
    pub(crate) fn start(&mut self,reducer:&Phase,_game:Option<&crate::games::PreparedGame>,captured:Option<Scope>)->Result<(),String>{
        if let Some(captured)=captured { self.inner.as_mut().ok_or("The desktop adapter is stopped.")?.bind_scope_before_start(captured.clone())?;self.scope=Some(captured); }
        self.started=true;
        if self.scope.is_none(){return Ok(());}
        self.inner.as_mut().ok_or("The desktop adapter is stopped.")?.start(reducer,None)
    }
    pub(crate) fn refresh_context(&mut self,scope:Option<Scope>)->Result<(),String>{
        if self.scope==scope{return Ok(());}
        // Complete release before replacing the immutable native process scope.
        if let Some(inner)=self.inner.as_mut(){inner.stop()?;}
        self.inner=None;
        // Inspect the clean journal using its immutable run identity. Its old
        // process scope is retired, then the replacement is durably bound.
        let mut replacement=Self::acquire(&self.owner,&self.program,None)?;
        if let Some(scope)=scope.as_ref(){replacement.inner.as_mut().unwrap().bind_scope_before_start(scope.clone())?;}
        if self.started&&scope.is_some(){replacement.inner.as_mut().unwrap().start_waiting_for_neutral()?;}
        self.inner=replacement.inner.take();self.scope=scope;
        Ok(())
    }
    pub(crate) fn poll(&mut self,frames:&[Frame],reducer:&Phase)->Result<Vec<String>,String>{
        // An accepted selected-game scope remains dormant until its process
        // exists. Waiting is inspectable state, without output or a health fault.
        if self.started&&self.scope.is_none(){return Ok(vec![]);}
        self.inner.as_mut().ok_or("The desktop adapter is stopped.")?.poll(frames,reducer)
    }
    pub(crate) fn keep_alive(&mut self)->Result<(),String>{self.inner.as_mut().map_or(Ok(()),|inner|inner.keep_alive())}
    pub(crate) fn stop(&mut self)->Result<(),String>{self.started=false;self.inner.as_mut().map_or(Ok(()),|inner|inner.stop())}
    pub(crate) fn status(&self)->Value{
        let mut status=self.inner.as_ref().map_or(serde_json::json!({"state":"stopped"}),|inner|inner.status());
        if self.started&&self.scope.is_none(){status["state"]=serde_json::json!("waiting");status["scopeError"]=serde_json::json!("Waiting for the profile's game to open.");}
        status
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn absent_selected_game_is_healthy_waiting_without_accessing_desktop_output(){
        let program=Program{sources:vec![],nodes:vec![],sinks:vec![],controllers:vec![]};
        let owner=RunOwner{session:"94a5ece4-4e3a-4395-a69b-3aa79c1ecf12".into(),generation:1,
            profile_id:"94a5ece4-4e3a-4395-a69b-3aa79c1ecf13".into(),profile_revision:1,game_id:Some("dcs".into())};
        let reducer=Phase::new(program.clone(),Counter(1),owner.session.clone()).unwrap();
        let mut desktop=Desktop{inner:None,owner,program,scope:None,started:true};
        assert!(desktop.poll(&[],&reducer).unwrap().is_empty());
        assert_eq!(desktop.status()["state"],"waiting");
        assert_eq!(desktop.status()["scopeError"],"Waiting for the profile's game to open.");
        assert!(desktop.inner.is_none(),"Waiting must not construct or activate an output adapter");
        desktop.started=false;
        assert!(desktop.poll(&[],&reducer).unwrap_err().contains("adapter is stopped"),"Actual adapter failures remain errors");
    }
    #[test]
    fn selected_game_without_a_native_process_cannot_fall_back_to_a_local_application(){
        let unrelated=crate::game_discovery::ProcessIdentity{pid:99,path:"C:\\Fixture\\Other.exe".into(),epoch:88};
        assert!(selected_game_scope(None).unwrap_err().contains("selected game"));
        let game=crate::game_discovery::ProcessIdentity{pid:42,path:"C:\\Fixture\\Game.exe".into(),epoch:10};
        let scope=selected_game_scope(Some(game.clone())).unwrap();assert_eq!(scope,Scope::Application{identity:identity(&game)});
        assert_ne!(scope,Scope::Application{identity:identity(&unrelated)});
    }
    #[test]
    fn saved_keyboard_scope_is_independent_of_machine_scope_and_other_profiles(){
        use osg_profiles::DesktopScope;
        let mut preferences=settings::Preferences::default();
        preferences.scope=settings::Scope::DesktopWide;
        let first=crate::game_discovery::ProcessIdentity{pid:42,path:"C:\\Fixture\\First.exe".into(),epoch:10};
        let second=crate::game_discovery::ProcessIdentity{pid:43,path:"C:\\Fixture\\Second.exe".into(),epoch:11};
        let first_reference="01234567-89ab-4cde-8fab-0123456789ab".to_string();
        let second_reference="11234567-89ab-4cde-8fab-0123456789ab".to_string();
        preferences.applications.insert(first_reference.clone(),first.value());
        preferences.applications.insert(second_reference.clone(),second.value());
        let first_scope=DesktopScope::Application{application_reference:Some(first_reference)};
        let second_scope=DesktopScope::Application{application_reference:Some(second_reference)};
        assert_eq!(prepare_scope(Some(&first_scope),false,Some(Some(second.clone())),&preferences).unwrap(),Some(Scope::Application{identity:identity(&first)}));
        assert_eq!(prepare_scope(Some(&second_scope),false,None,&preferences).unwrap(),Some(Scope::Application{identity:identity(&second)}));
        assert_eq!(prepare_scope(Some(&first_scope),true,Some(None),&preferences).unwrap(),Some(Scope::Application{identity:identity(&first)}),"launch preserves explicit application scope");
        assert_eq!(prepare_scope(Some(&DesktopScope::DesktopWide),false,None,&preferences).unwrap(),Some(Scope::DesktopWide));
        assert!(prepare_scope(None,false,None,&preferences).is_err(),"legacy machine desktop-wide preference cannot broaden an old profile");
        assert!(prepare_scope(Some(&DesktopScope::Application{application_reference:None}),false,Some(Some(first.clone())),&preferences).is_err(),"unselected application does not inherit another context");
        preferences.applications.clear();
        assert!(prepare_scope(Some(&first_scope),false,None,&preferences).is_err(),"foreign/imported app references remain unavailable");
    }
    #[test]
    fn selected_game_scope_waits_for_closed_game_and_never_inherits_an_unrelated_application(){
        use osg_profiles::DesktopScope;
        let mut preferences=settings::Preferences::default();
        let game=crate::game_discovery::ProcessIdentity{pid:42,path:"C:\\Fixture\\Game.exe".into(),epoch:10};
        preferences.application=Some(crate::game_discovery::ProcessIdentity{pid:99,path:"C:\\Fixture\\Other.exe".into(),epoch:88}.value());
        assert_eq!(prepare_scope(Some(&DesktopScope::SelectedGame),true,Some(None),&preferences).unwrap(),None);
        assert_eq!(prepare_scope(Some(&DesktopScope::SelectedGame),true,None,&preferences).unwrap(),None,"Hardware-first launch has no process context yet");
        assert_eq!(prepare_scope(Some(&DesktopScope::SelectedGame),false,Some(Some(game.clone())),&preferences).unwrap(),Some(Scope::Application{identity:identity(&game)}));
        assert_eq!(prepare_scope(Some(&DesktopScope::SelectedGame),false,Some(None),&preferences).unwrap(),None);
        assert!(prepare_scope(Some(&DesktopScope::SelectedGame),false,None,&preferences).is_err());
        assert_eq!(prepare_scope(None,false,Some(None),&preferences).unwrap(),None);
        assert!(matches!(prepare_scope(None,false,None,&preferences).unwrap(),Some(Scope::Application{..})),"legacy captured applications retain application-only scope");
    }
}

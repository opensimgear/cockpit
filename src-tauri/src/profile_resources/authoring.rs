//! Saves validate retained native identity, independently of live presence.
use super::*;

impl Runtime {
    pub(crate) fn validate_profile_save(
        &self,
        document: &osg_profiles::ProfileDocument,
        assignments: &[osg_profiles::store::LocalAssignment],
    ) -> Result<(), String> {
        use osg_profiles::resources::ResourceType;
        use osg_profiles::ResourceKind;
        let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if state.read_only && document.resources.iter().any(|resource| resource.kind != ResourceKind::DebugController && assignments.iter().any(|pin|pin.resource_id==resource.id)) {
            return Err("Resource inventory could not be verified. Resolve its diagnostics before saving assigned components.".into());
        }
        let mut broken = Vec::new();
        for resource in &document.resources {
            if resource.kind == ResourceKind::DebugController { continue; }
            let Some(assignment) = assignments.iter().find(|pin| pin.resource_id == resource.id) else { continue; };
            let matches = state.registry.inspect_selected(&assignment.source_id, &[]).ok().is_some_and(|details| {
                matches!((resource.kind, details.resource.resource_type),
                    (ResourceKind::Device, ResourceType::HostController | ResourceType::OsgModule)
                    | (ResourceKind::Companion, ResourceType::Companion)
                    | (ResourceKind::Game, ResourceType::GameIntegration))
            });
            if !matches { broken.push(resource.name.as_str()); }
        }
        if broken.is_empty() { Ok(()) } else {
            Err(format!("Remove broken components before saving: {}.", broken.join(", ")))
        }
    }
}

#[cfg(test)]
impl Runtime {
    /// The large-envelope fixture needs retained identities, rather than saved
    /// references to nonexistent hardware. Seeding metadata performs no discovery.
    pub(crate) fn retain_fixture_assignments(&self, request: &mut Value) {
        use osg_profiles::resources::ResourceType;
        let mut state=self.state.lock().unwrap();
        let observations=request["document"]["resources"].as_array().unwrap().iter().map(|resource|Observation {
            key:resource["id"].as_str().unwrap().into(),name:resource["name"].as_str().unwrap().into(),resource_type:ResourceType::HostController,
            evidence:vec![],attributes:BTreeMap::new(),diagnostics:vec![],route:None,ports:None,
        }).collect();
        state.registry.reconcile(batch("save-boundary-fixture","save-boundary-session",1,observations)).unwrap();
        let ids=state.registry.list(0,128).unwrap().resources.into_iter().map(|resource|(resource.name,resource.id)).collect::<BTreeMap<_,_>>();
        let names=request["document"]["resources"].as_array().unwrap().iter().map(|resource|(resource["id"].as_str().unwrap().to_owned(),resource["name"].as_str().unwrap().to_owned())).collect::<BTreeMap<_,_>>();
        for assignment in request["assignments"].as_array_mut().unwrap() {
            assignment["sourceId"]=json!(ids[&names[assignment["resourceId"].as_str().unwrap()]]);
        }
    }
}

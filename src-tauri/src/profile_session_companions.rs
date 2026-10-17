//! Profile-owned panel assignments, native input feeds and typed widget state.
//! No global feedback route store or browser input participates in this adapter.
use osg_companion_service::{
    profile::{AssignmentReceipt, Preparation},
    FeedbackBinding, FeedbackDescriptor, FeedbackKind, FeedbackMode, FeedbackProvenance,
    FeedbackSample, FeedbackSource, FeedbackStatus, FeedbackValue, Hub,
};
use osg_mapping::{
    engine::{
        typed::{Change, Frame, Phase},
        Counter, Reason, RunToken,
    },
    program::{Endpoint, SignalSample, SignalValue},
    signal::PortValue,
};
use osg_mapping_host::companions::{
    native_definition, CompanionProvider, Definition, NativeFeed, NativeSelection,
};
use osg_profiles::{
    compiler_models::{ExecutionPlan, PlanReadiness, SinkAdapter},
    resources::{Access, ResourceToken, Transport},
    resources_ports::{PortContract, PortDescriptor},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

pub(crate) struct PreparedCompanion {
    pub resource_id: String,
    pub proof: Preparation,
}
struct Input {
    logical: String,
    observed: String,
    resource: String,
    descriptor: PortDescriptor,
    token: Option<ResourceToken>,
}
struct Output {
    endpoint: osg_profiles::Endpoint,
    from: Endpoint,
    observed: String,
    resource: String,
    descriptor: PortDescriptor,
    binding: FeedbackBinding,
    feedback: FeedbackDescriptor,
    token: Option<ResourceToken>,
}
struct Instance {
    logical: String,
    proof: Preparation,
    provider_session: Option<String>,
    transport: Option<Transport>,
    definition: Definition,
    inputs: Vec<Input>,
    outputs: Vec<Output>,
    receipt: Option<AssignmentReceipt>,
    feed: Option<NativeFeed>,
    generation: u64,
    sequence: u64,
    at: u64,
    ready: bool,
    feedback_at: Option<u64>,
    feedback_live: bool,
}
pub(crate) struct CompanionSources {
    instances: Vec<Instance>,
    owner: String,
    acquired: bool,
    stopped: bool,
    run: Option<RunToken>,
}
#[derive(Default)]
pub(crate) struct CompanionPoll {
    pub frames: Vec<Frame>,
    pub diagnostics: Vec<String>,
}

fn selected_port(
    ports: &[PortDescriptor],
    observed: Option<&String>,
    contract: &PortContract,
) -> Result<PortDescriptor, String> {
    ports
        .iter()
        .find(|port| Some(&port.id) == observed && &port.contract == contract)
        .cloned()
        .ok_or("The companion port does not match the retained immutable native contract.".into())
}
fn token(
    token: Option<&ResourceToken>,
    resource: &str,
    descriptor: &PortDescriptor,
    instance: &str,
    access: Access,
) -> Result<ResourceToken, String> {
    let token = token
        .ok_or("Acquire a current actual companion delivery token before binding its signal.")?;
    if token.resource_id != resource
        || token.port_id != descriptor.id
        || token.contract_hash != descriptor.contract_hash()?
        || token.route_id != format!("companion:{instance}")
        || token.access != access
    {
        return Err("The companion delivery token changed identity, contract or access.".into());
    }
    Ok(token.clone())
}
impl CompanionSources {
    pub(crate) fn prepare(
        plan: &ExecutionPlan,
        bindings: Vec<PreparedCompanion>,
    ) -> Result<Self, String> {
        if bindings.len() > 8 {
            return Err("A profile may own at most eight companion instances.".into());
        }
        let mut logical_ids = BTreeSet::new();
        let mut instances = BTreeSet::new();
        let mut result = vec![];
        for binding in bindings {
            if !logical_ids.insert(binding.resource_id.clone())
                || !instances.insert(binding.proof.instance_id().to_owned())
                || !plan.companion_assignments.iter().any(|assignment| {
                    assignment.resource_id == binding.resource_id
                        && assignment.publication_hash == binding.proof.publication_hash()
                })
            {
                return Err("Companion preparation must match each explicit profile instance/publication once.".into());
            }
            let definition = native_definition(binding.proof.definition())?;
            let ports =
                crate::profile_resource_ports::companion(&definition, binding.proof.feedback())?;
            let mut inputs = vec![];
            for source in plan.sources.iter().filter(|source| {
                source.endpoint.node_id == binding.resource_id
                    && source.readiness == PlanReadiness::Ready
            }) {
                let descriptor =
                    selected_port(&ports, source.observed_port_id.as_ref(), &source.contract)?;
                if descriptor.direction != osg_profiles::Direction::Output {
                    return Err("Companion input must use an advertised output port.".into());
                }
                inputs.push(Input {
                    logical: source.endpoint.port_id.clone(),
                    observed: descriptor.id.clone(),
                    resource: source
                        .inventory_resource_id
                        .clone()
                        .ok_or("The companion source has no native identity.")?,
                    descriptor,
                    token: None,
                });
            }
            let mut outputs = vec![];
            let mut channels = BTreeSet::new();
            for sink in plan.sinks.iter().filter(|sink| {
                sink.endpoint.node_id == binding.resource_id
                    && sink.readiness == PlanReadiness::Ready
                    && sink.adapter == SinkAdapter::CompanionFeedback
            }) {
                let descriptor =
                    selected_port(&ports, sink.observed_port_id.as_ref(), &sink.contract)?;
                let selected = binding
                    .proof
                    .feedback()
                    .iter()
                    .flat_map(|feedback| {
                        feedback
                            .output_ids
                            .iter()
                            .map(move |output| (feedback, output))
                    })
                    .find(|(feedback, output)| {
                        descriptor.id == format!("feedback:{}:{output}", feedback.channel_id)
                    })
                    .ok_or("The immutable panel does not advertise this feedback output.")?;
                if !channels.insert(selected.0.channel_id.clone()) {
                    return Err("Bind each published companion feedback channel once.".into());
                }
                let from = plan
                    .program
                    .sinks
                    .iter()
                    .find(|program_sink| {
                        program_sink.endpoint.node_id == sink.endpoint.node_id
                            && program_sink.endpoint.port_id == sink.endpoint.port_id
                    })
                    .ok_or("The companion feedback branch is missing its visible graph binding.")?
                    .from
                    .clone();
                outputs.push(Output {
                    endpoint: sink.endpoint.clone(),
                    from,
                    observed: descriptor.id.clone(),
                    resource: sink
                        .inventory_resource_id
                        .clone()
                        .ok_or("The feedback target has no exact native identity.")?,
                    descriptor,
                    binding: FeedbackBinding {
                        channel_id: selected.0.channel_id.clone(),
                        output_id: selected.1.clone(),
                    },
                    feedback: selected.0.clone(),
                    token: None,
                });
            }
            result.push(Instance {
                logical: binding.resource_id.clone(),
                transport: plan
                    .sources
                    .iter()
                    .filter(|s| s.endpoint.node_id == binding.resource_id)
                    .filter_map(|s| s.route.as_ref())
                    .chain(
                        plan.sinks
                            .iter()
                            .filter(|s| s.endpoint.node_id == binding.resource_id)
                            .filter_map(|s| s.route.as_ref()),
                    )
                    .next()
                    .map(|route| route.transport),
                provider_session: plan
                    .sources
                    .iter()
                    .filter(|s| s.endpoint.node_id == binding.resource_id)
                    .filter_map(|s| s.preparation.as_ref())
                    .chain(
                        plan.sinks
                            .iter()
                            .filter(|s| s.endpoint.node_id == binding.resource_id)
                            .filter_map(|s| s.preparation.as_ref()),
                    )
                    .next()
                    .map(|p| p.provider_session().to_owned()),
                proof: binding.proof,
                definition,
                inputs,
                outputs,
                receipt: None,
                feed: None,
                generation: 1,
                sequence: 0,
                at: 0,
                ready: false,
                feedback_at: None,
                feedback_live: false,
            });
        }
        for source in plan.sources.iter().filter(|source| {
            source.readiness == PlanReadiness::Ready
                && source
                    .route
                    .as_ref()
                    .is_some_and(|route| route.id.starts_with("companion:"))
        }) {
            if !logical_ids.contains(&source.endpoint.node_id) {
                return Err(
                    "Prepare the selected companion publication before admitting its input.".into(),
                );
            }
        }
        for sink in plan.sinks.iter().filter(|sink| {
            sink.readiness == PlanReadiness::Ready && sink.adapter == SinkAdapter::CompanionFeedback
        }) {
            if !logical_ids.contains(&sink.endpoint.node_id) {
                return Err(
                    "Prepare the selected companion publication before admitting its feedback."
                        .into(),
                );
            }
        }
        Ok(Self {
            instances: result,
            owner: uuid::Uuid::new_v4().to_string(),
            acquired: false,
            stopped: false,
            run: None,
        })
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.instances.is_empty()
    }
    pub(crate) fn validate_preparation(
        &self,
        runtime: &crate::companion_host::Runtime,
    ) -> Result<(), String> {
        if self.is_empty() {
            return Ok(());
        }
        let hub = runtime.profile_metadata()?;
        for instance in &self.instances {
            if self.acquired {
                if self.stopped {
                    return Err("The owned companion assignment is stopped.".into());
                }
                hub.validate_profile_assignment(
                    instance
                        .receipt
                        .as_ref()
                        .ok_or("The owned companion receipt is missing.")?,
                )?;
            } else {
                hub.validate_profile_preparation(&instance.proof)?;
            }
        }
        Ok(())
    }
    pub(crate) fn arm(&mut self, run: RunToken) -> Result<(), String> {
        if !self.acquired || self.stopped || self.run.is_some() {
            return Err(
                "The companion adapter cannot arm before acquisition or more than once.".into(),
            );
        }
        if self.instances.iter().any(|instance| {
            !instance.ready
                || instance.inputs.iter().any(|input| input.token.is_none())
                || instance.outputs.iter().any(|output| output.token.is_none())
        }) {
            return Err(
                "Bind exact companion delivery tokens and capture fresh baselines before Start."
                    .into(),
            );
        }
        self.run = Some(run);
        Ok(())
    }
    pub(crate) fn capture(
        &self,
        hub: &Hub,
        previous: &[AssignmentReceipt],
    ) -> Result<Vec<AssignmentReceipt>, String> {
        self.instances
            .iter()
            .map(|instance| {
                hub.capture_profile_assignment(
                    &instance.proof,
                    &self.owner,
                    previous
                        .iter()
                        .find(|receipt| receipt.instance_id() == instance.proof.instance_id()),
                )
            })
            .collect()
    }
    pub(crate) fn acquire(
        &mut self,
        runtime: &crate::companion_host::Runtime,
        receipts: &[AssignmentReceipt],
    ) -> Result<(), String> {
        if self.instances.is_empty() {
            self.acquired = true;
            return Ok(());
        }
        let hub = runtime.profile_acquire_transports()?;
        self.acquire_native(&hub, runtime.profile_provider(), receipts)
    }
    fn acquire_native(
        &mut self,
        hub: &Hub,
        provider: Arc<CompanionProvider>,
        receipts: &[AssignmentReceipt],
    ) -> Result<(), String> {
        if self.acquired || self.stopped {
            return Err("Prepare a fresh companion reader for each profile takeover.".into());
        }
        for instance in &mut self.instances {
            let receipt = receipts
                .iter()
                .find(|receipt| {
                    receipt.instance_id() == instance.proof.instance_id()
                        && receipt.owner() == self.owner
                        && receipt.publication_hash() == instance.proof.publication_hash()
                        && receipt.capability_fingerprint()
                            == instance.proof.capability_fingerprint()
                })
                .ok_or("The journal is missing the exact incoming companion assignment owner.")?;
            hub.acquire_profile_assignment(receipt)?;
            instance.receipt = Some(receipt.clone());
            hub.configure_profile_feedback(
                receipt,
                &instance
                    .outputs
                    .iter()
                    .map(|output| output.binding.clone())
                    .collect::<Vec<_>>(),
            )?;
            instance.feed = Some(
                provider.subscribe_native(&[NativeSelection {
                    definition: instance.definition.clone(),
                    controls: instance
                        .definition
                        .controls
                        .iter()
                        .map(|control| control.id.clone())
                        .collect(),
                }])?,
            );
        }
        self.acquired = true;
        Ok(())
    }
    /// Native baseline discovery without granting a token or feeding the reducer.
    /// The Host may publish this exact owned observation, bind real tokens, then
    /// capture another fresh baseline immediately before Start.
    pub(crate) fn baseline_ready(&mut self, hub: &Hub) -> Result<bool, String> {
        if !self.acquired || self.stopped {
            return Err("The companion reader is not acquired.".into());
        }
        for instance in &mut self.instances {
            hub.validate_profile_assignment(
                instance
                    .receipt
                    .as_ref()
                    .ok_or("The companion owner is missing.")?,
            )?;
            if instance
                .feed
                .as_mut()
                .ok_or("The companion subscription is missing.")?
                .baseline()
                .is_err()
            {
                return Ok(false);
            }
        }
        Ok(true)
    }
    pub(crate) fn bind_delivery_plan(&mut self, delivery: &ExecutionPlan) -> Result<(), String> {
        let bindings = self
            .instances
            .iter()
            .map(|instance| instance.delivery_bindings(delivery))
            .collect::<Result<Vec<_>, _>>()?;
        for (instance, (inputs, outputs)) in self.instances.iter_mut().zip(bindings) {
            instance.install_bindings(inputs, outputs);
        }
        Ok(())
    }
    pub(crate) fn needs_rebind(
        &self,
        validate: impl Fn(&ResourceToken) -> Result<(), String>,
    ) -> bool {
        self.instances.iter().any(|i| {
            !i.ready
                || i.inputs
                    .iter()
                    .any(|p| p.token.as_ref().is_none_or(|t| validate(t).is_err()))
                || i.outputs
                    .iter()
                    .any(|p| p.token.as_ref().is_none_or(|t| validate(t).is_err()))
        })
    }
    /// Only exact already-owned instances may advance delivery generations.
    /// One missing peer cannot prevent another from obtaining a fresh baseline.
    pub(crate) fn rebind(
        &mut self,
        hub: &Hub,
        delivery: &ExecutionPlan,
        allowed: impl Fn(&str) -> bool,
        validate: impl Fn(&ResourceToken) -> Result<(), String>,
        now: u64,
    ) -> CompanionPoll {
        let mut poll = CompanionPoll::default();
        if !self.acquired || self.stopped {
            return poll;
        }
        for instance in &mut self.instances {
            if !allowed(&instance.logical) {
                continue;
            }
            let result = (|| {
                let (inputs, outputs) = instance.delivery_bindings(delivery)?;
                if outputs.iter().any(Option::is_none) {
                    return Err("The companion feedback has no current write capability.".into());
                }
                for token in inputs.iter().chain(outputs.iter().flatten()) {
                    validate(token)?;
                }
                let changed = instance
                    .inputs
                    .iter()
                    .map(|p| p.token.as_ref())
                    .ne(inputs.iter().map(Some))
                    || instance
                        .outputs
                        .iter()
                        .map(|p| p.token.as_ref())
                        .ne(outputs.iter().map(Option::as_ref));
                if instance.ready && !changed {
                    return Ok(vec![]);
                }
                if instance.ready {
                    if let Some(frame) = instance.loss(now)? {
                        poll.frames.push(frame);
                    }
                    if let Some(receipt) = &instance.receipt {
                        hub.clear_profile_feedback(receipt, hub.now_ms())?;
                    }
                    instance.feedback_live = false;
                }
                instance.install_bindings(inputs, outputs);
                instance.validate_inputs(hub, &validate)?;
                let frames = instance
                    .feed
                    .as_mut()
                    .ok_or("The companion subscription is unavailable.")?
                    .baseline()?;
                let mut result = vec![];
                for frame in frames {
                    if let Some(frame) = instance.convert(frame, now)? {
                        result.push(frame);
                    }
                }
                Ok::<_, String>(result)
            })();
            match result {
                Ok(frames) => poll.frames.extend(frames),
                Err(error) => poll
                    .diagnostics
                    .push(format!("{}: {error}", instance.logical)),
            }
        }
        poll
    }
    pub(crate) fn fail(&mut self, now: u64, error: String) -> CompanionPoll {
        let mut result = CompanionPoll {
            frames: vec![],
            diagnostics: vec![error],
        };
        for instance in &mut self.instances {
            if instance.ready {
                if let Ok(Some(frame)) = instance.loss(now) {
                    result.frames.push(frame);
                }
            }
            instance.feedback_live = false;
        }
        result
    }
    pub(crate) fn stop_native(
        &mut self,
        runtime: &crate::companion_host::Runtime,
    ) -> Result<(), String> {
        if self.is_empty() {
            self.stopped = true;
            self.run = None;
            return Ok(());
        }
        match runtime.profile_metadata() {
            Ok(hub) => self.stop(&hub),
            Err(error) => {
                self.stopped = true;
                self.run = None;
                for i in &mut self.instances {
                    i.feed.take();
                    i.ready = false;
                }
                Err(error)
            }
        }
    }
    pub(crate) fn baseline(
        &mut self,
        hub: &Hub,
        validate: impl Fn(&ResourceToken) -> Result<(), String>,
        now_us: u64,
    ) -> Result<Option<Vec<Frame>>, String> {
        if !self.baseline_ready(hub)? {
            return Ok(None);
        }
        let mut baselines = vec![];
        for instance in &mut self.instances {
            instance.validate_inputs(hub, &validate)?;
            baselines.push(instance.feed.as_mut().unwrap().baseline()?);
        }
        if self.run.is_none() {
            // A genuine peer can reconnect after inventory capture but before
            // this neutral baseline is read. Keep acquisition passive until
            // the Host refreshes and binds that exact new native generation.
            // Check every source before conversion changes any instance state.
            let mut generation_changed = false;
            for (instance, frames) in self.instances.iter().zip(&baselines) {
                for frame in frames {
                    if frame.source.id != instance.proof.instance_id() {
                        return Err(
                            "The companion frame came from another physical instance.".into()
                        );
                    }
                    if instance
                        .inputs
                        .iter()
                        .map(|input| &input.token)
                        .chain(instance.outputs.iter().map(|output| &output.token))
                        .flatten()
                        .any(|token| token.provider_generation != frame.generation.0.to_string())
                    {
                        generation_changed = true;
                    }
                }
            }
            if generation_changed {
                return Ok(None);
            }
        }
        let mut result = vec![];
        for (instance, frames) in self.instances.iter_mut().zip(baselines) {
            for frame in frames {
                if let Some(frame) = instance.convert(frame, now_us)? {
                    result.push(frame);
                }
            }
        }
        Ok(Some(result))
    }
    pub(crate) fn poll(
        &mut self,
        hub: &Hub,
        validate: impl Fn(&ResourceToken) -> Result<(), String>,
        now_us: u64,
    ) -> CompanionPoll {
        let mut result = CompanionPoll::default();
        if !self.acquired || self.stopped {
            return result;
        }
        for instance in &mut self.instances {
            let was_ready = instance.ready;
            let outcome = (|| {
                instance.validate_inputs(hub, &validate)?;
                let frames = instance
                    .feed
                    .as_mut()
                    .ok_or("The companion subscription is unavailable.")?
                    .poll()?;
                let mut converted = vec![];
                for frame in frames {
                    if let Some(frame) = instance.convert(frame, now_us)? {
                        converted.push(frame);
                    }
                }
                Ok::<_, String>(converted)
            })();
            match outcome {
                Ok(frames) => result.frames.extend(frames),
                Err(error) => {
                    // A discarded batch may already have converted a Gap and
                    // cleared ready before a later frame failed. Preserve its
                    // loss fence even though none of that batch is delivered.
                    let clear = was_ready || instance.ready || instance.feedback_live;
                    if was_ready || instance.ready {
                        if let Ok(frame) = instance.loss(now_us) {
                            result.frames.extend(frame);
                        }
                    }
                    result
                        .diagnostics
                        .push(format!("{}: {error}", instance.logical));
                    if clear {
                        if let Some(receipt) = &instance.receipt {
                            let _ = hub.clear_profile_feedback(receipt, hub.now_ms());
                        }
                    }
                    instance.feedback_live = false;
                }
            }
        }
        result
    }
    pub(crate) fn publish_feedback(
        &mut self,
        hub: &Hub,
        phase: &Phase,
        validate: impl Fn(&ResourceToken) -> Result<(), String>,
        now_us: u64,
    ) -> Vec<String> {
        let mut diagnostics = vec![];
        if self.stopped || !self.acquired {
            return diagnostics;
        }
        if !phase.active()
            || self.run.as_ref() != Some(&phase.run_token())
            || phase.at_us().0 < now_us
        {
            return vec![
                "The companion feedback reducer run is not armed by this native owner.".into(),
            ];
        }
        for instance in &mut self.instances {
            if !instance.ready
                || instance.outputs.is_empty()
                || instance
                    .feedback_at
                    .is_some_and(|last| now_us.saturating_sub(last) < 34_000)
            {
                continue;
            }
            let result = (|| {
                let receipt = instance
                    .receipt
                    .as_ref()
                    .ok_or("The companion feedback owner is missing.")?;
                hub.validate_profile_assignment(receipt)?;
                for output in &instance.outputs {
                    validate(
                        output
                            .token
                            .as_ref()
                            .ok_or("Bind current companion feedback delivery tokens first.")?,
                    )?;
                }
                let samples = instance
                    .outputs
                    .iter()
                    .map(|output| feedback_sample(output, phase.sample(&output.from)))
                    .collect::<Result<Vec<_>, _>>()?;
                let token = hub.profile_feedback_token(receipt)?;
                hub.publish_profile_feedback(
                    receipt,
                    &token,
                    &samples,
                    FeedbackMode::Snapshot,
                    hub.now_ms(),
                )?;
                Ok::<_, String>(())
            })();
            instance.feedback_at = Some(now_us);
            match result {
                Ok(()) => instance.feedback_live = true,
                Err(error) => {
                    if instance.feedback_live {
                        if let Some(receipt) = &instance.receipt {
                            let _ = hub.clear_profile_feedback(receipt, hub.now_ms());
                        }
                    }
                    instance.feedback_live = false;
                    diagnostics.push(format!("{} feedback: {error}", instance.logical));
                }
            }
        }
        diagnostics
    }
    pub(crate) fn stop(&mut self, hub: &Hub) -> Result<(), String> {
        self.stopped = true;
        self.run = None;
        let mut errors = vec![];
        for instance in &mut self.instances {
            instance.feed.take();
            instance.ready = false;
            if let Some(receipt) = &instance.receipt {
                if hub.validate_profile_assignment_owner(receipt).is_ok() {
                    if let Err(error) = hub.clear_profile_feedback(receipt, hub.now_ms()) {
                        errors.push(error);
                    }
                }
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }
}
impl Instance {
    fn delivery_bindings(
        &self,
        delivery: &ExecutionPlan,
    ) -> Result<(Vec<ResourceToken>, Vec<Option<ResourceToken>>), String> {
        let mut inputs = vec![];
        let mut outputs = vec![];
        for port in &self.inputs {
            let source = delivery
                .sources
                .iter()
                .find(|source| {
                    source.endpoint.node_id == self.logical
                        && source.endpoint.port_id == port.logical
                        && source.observed_port_id.as_ref() == Some(&port.observed)
                        && source.inventory_resource_id.as_ref() == Some(&port.resource)
                        && source.contract == port.descriptor.contract
                        && source.readiness == PlanReadiness::Ready
                })
                .ok_or("The companion input binding changed before delivery.")?;
            if self.transport.is_some_and(|transport| {
                source
                    .route
                    .as_ref()
                    .is_none_or(|route| route.transport != transport)
            }) {
                return Err(
                    "The companion transport changed; prepare a fresh native assignment.".into(),
                );
            }
            inputs.push(token(
                source.token.as_ref(),
                &port.resource,
                &port.descriptor,
                self.proof.instance_id(),
                Access::Read,
            )?);
        }
        for port in &self.outputs {
            let sink = delivery
                .sinks
                .iter()
                .find(|sink| {
                    sink.endpoint == port.endpoint
                        && sink.adapter == SinkAdapter::CompanionFeedback
                        && sink.observed_port_id.as_ref() == Some(&port.observed)
                        && sink.inventory_resource_id.as_ref() == Some(&port.resource)
                        && sink.contract == port.descriptor.contract
                        && sink.readiness == PlanReadiness::Ready
                })
                .ok_or("The companion feedback binding changed before delivery.")?;
            if self.transport.is_some_and(|transport| {
                sink.route
                    .as_ref()
                    .is_none_or(|route| route.transport != transport)
            }) {
                return Err(
                    "The companion transport changed; prepare a fresh native assignment.".into(),
                );
            }
            outputs.push(
                sink.token
                    .as_ref()
                    .map(|delivery| {
                        token(
                            Some(delivery),
                            &port.resource,
                            &port.descriptor,
                            self.proof.instance_id(),
                            Access::Write,
                        )
                    })
                    .transpose()?,
            );
        }
        let session = self
            .provider_session
            .as_deref()
            .or_else(|| inputs.first().map(|t| t.provider_session.as_str()))
            .or_else(|| {
                outputs
                    .iter()
                    .flatten()
                    .next()
                    .map(|t| t.provider_session.as_str())
            });
        if inputs
            .iter()
            .chain(outputs.iter().flatten())
            .any(|t| Some(t.provider_session.as_str()) != session)
        {
            return Err(
                "The companion transport provider changed; prepare a fresh native assignment."
                    .into(),
            );
        }
        Ok((inputs, outputs))
    }
    fn install_bindings(
        &mut self,
        inputs: Vec<ResourceToken>,
        outputs: Vec<Option<ResourceToken>>,
    ) {
        if self.provider_session.is_none() {
            self.provider_session = inputs
                .first()
                .or_else(|| outputs.iter().flatten().next())
                .map(|t| t.provider_session.clone());
        }
        for (port, token) in self.inputs.iter_mut().zip(inputs) {
            port.token = Some(token);
        }
        for (port, token) in self.outputs.iter_mut().zip(outputs) {
            port.token = token;
        }
    }
    fn validate_inputs(
        &self,
        hub: &Hub,
        validate: &impl Fn(&ResourceToken) -> Result<(), String>,
    ) -> Result<(), String> {
        hub.validate_profile_assignment(
            self.receipt
                .as_ref()
                .ok_or("The companion assignment is not acquired.")?,
        )?;
        for input in &self.inputs {
            validate(
                input
                    .token
                    .as_ref()
                    .ok_or("Bind an actual fresh companion read token first.")?,
            )?;
        }
        for output in &self.outputs {
            if let Some(token) = &output.token {
                validate(token)?;
            }
        }
        Ok(())
    }
    fn next(&mut self) -> Result<u64, String> {
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or("Companion reducer sequence exhausted.")?;
        Ok(self.sequence)
    }
    fn loss(&mut self, now_us: u64) -> Result<Option<Frame>, String> {
        self.ready = false;
        self.at = self.at.max(now_us);
        if self.inputs.is_empty() {
            return Ok(None);
        }
        Ok(Some(Frame {
            source: self.logical.clone(),
            generation: Counter(self.generation),
            sequence: Counter(self.next()?),
            captured_us: Counter(self.at),
            reason: Reason::Disconnect,
            changes: vec![],
        }))
    }
    fn convert(
        &mut self,
        frame: osg_mapping::engine::Frame,
        now_us: u64,
    ) -> Result<Option<Frame>, String> {
        if frame.source.id != self.proof.instance_id() {
            return Err("The companion frame came from another physical instance.".into());
        }
        if matches!(frame.reason, Reason::Disconnect | Reason::Gap) {
            return self.loss(now_us.max(frame.captured_us.0));
        }
        if self
            .inputs
            .iter()
            .map(|input| &input.token)
            .chain(self.outputs.iter().map(|output| &output.token))
            .flatten()
            .any(|token| token.provider_generation != frame.generation.0.to_string())
        {
            return Err(
                "The companion input generation changed; bind a fresh native baseline.".into(),
            );
        }
        if frame.captured_us.0 < self.at {
            return Err("The native companion frame predates its last admitted sample.".into());
        }
        let baseline = frame.reason == Reason::Baseline;
        if !baseline && !self.ready {
            return Err("A complete fresh companion baseline is required after loss.".into());
        }
        self.generation = frame.generation.0;
        self.at = frame.captured_us.0;
        let mut changes = vec![];
        for input in &self.inputs {
            if let Some(change) = frame
                .changes
                .iter()
                .find(|change| change.control == input.observed)
            {
                let value = match change.value {
                    osg_mapping::profile::Value::Button(value) => PortValue::Boolean(value),
                    osg_mapping::profile::Value::Axis(value) => PortValue::Number(value as f64),
                    osg_mapping::profile::Value::Hat(value) => {
                        PortValue::Number(value.map_or(-1, i32::from) as f64)
                    }
                };
                changes.push(Change {
                    port_id: input.logical.clone(),
                    sample: SignalSample::scalar(value),
                });
            } else if baseline {
                return Err("The native companion baseline omitted a selected port.".into());
            }
        }
        self.ready = true;
        if self.inputs.is_empty() || (!baseline && changes.is_empty()) {
            return Ok(None);
        }
        Ok(Some(Frame {
            source: self.logical.clone(),
            generation: Counter(self.generation),
            sequence: Counter(self.next()?),
            captured_us: Counter(self.at),
            reason: if baseline {
                Reason::Baseline
            } else {
                Reason::Change
            },
            changes,
        }))
    }
}
fn feedback_sample(
    output: &Output,
    sample: Option<&SignalSample>,
) -> Result<FeedbackSample, String> {
    let (value, status) = match sample {
        Some(SignalSample::Known(SignalValue::Scalar(value))) => (
            Some(match value {
                PortValue::Boolean(value) => FeedbackValue::Boolean(*value),
                PortValue::Number(value) => FeedbackValue::Number(*value),
                PortValue::Text(value) => FeedbackValue::Text(value.clone()),
            }),
            FeedbackStatus::Fresh,
        ),
        Some(SignalSample::Known(_)) => {
            return Err("Companion feedback accepts typed scalar state, not an action pack.".into())
        }
        Some(SignalSample::Stale) => (None, FeedbackStatus::Stale),
        Some(SignalSample::Unavailable) => (None, FeedbackStatus::Disconnected),
        _ => (None, FeedbackStatus::Unknown),
    };
    Ok(FeedbackSample {
        channel_id: output.binding.channel_id.clone(),
        output_id: output.binding.output_id.clone(),
        value,
        status,
        provenance: FeedbackProvenance::Local,
        source_kind: FeedbackSource::Host,
        source_age_ms: 0,
        unit: output.feedback.unit.clone().unwrap_or_else(|| {
            if output.feedback.kind == FeedbackKind::Text {
                "text".into()
            } else {
                "state".into()
            }
        }),
        range: output.feedback.range.clone(),
    })
}

#[cfg(test)]
#[path = "profile_session_companions_tests.rs"]
mod tests;

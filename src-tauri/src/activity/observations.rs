use super::{capture, NewActivityEvent, Snapshot};
use std::{collections::BTreeMap, time::Instant};

pub(super) struct Observation {
    pub event: NewActivityEvent,
    pub subject: String,
    pub state: String,
    pub observed_at:Instant,
}
pub(super) struct Batch {
    pub started: Instant,
    pub states: BTreeMap<String,String>,
    pub events: Vec<Observation>,
}
impl Batch {
    pub fn poll(before:&Snapshot,after:&Snapshot,started:Instant)->Self {
        let kinds=["profile","game","game_process","game_session","game_vehicle","integration","discovery","gateway","module","controller","companion"];
        let available=kinds.into_iter().filter(|kind|after.available(kind)).collect::<Vec<_>>();
        Self::between(before,after,started).acknowledge(&available)
    }
    pub fn acknowledge(mut self,kinds:&[&str])->Self {
        for kind in kinds {self.states.insert(format!("@{kind}"),String::new());}
        self
    }
    pub fn between(before:&Snapshot,after:&Snapshot,started:Instant)->Self {
        Self::events(capture::diff(before,after),after,started)
    }
    pub fn events(events:Vec<NewActivityEvent>,after:&Snapshot,started:Instant)->Self {
        let states=capture::states(after);
        let events=events.into_iter().map(|event| {
            let kind=["profile_session","game_process","game_session","game_vehicle","discovery","integration","profile","game","gateway","module","controller","companion"]
                .into_iter().find(|kind|event.event_type.starts_with(&format!("{kind}_"))).expect("known native activity subject");
            let id=if matches!(kind,"profile_session"|"discovery"){"current"}else{event.entity_id.as_deref().unwrap_or("")};
            let subject=format!("{kind}:{id}");
            let state=capture::state(after,&subject).unwrap_or_else(||"removed".into());
            let observed_at=after.observed_at(&subject).unwrap_or(started);
            Observation{event,subject,state,observed_at}
        }).collect();
        Self{started,states,events}
    }
}

use super::*;
impl Registry {
    pub fn stream_snapshot(&self, ids: &[String]) -> Result<Snapshot, String> {
        if ids.is_empty()
            || ids.len() > 16
            || ids.iter().collect::<BTreeSet<_>>().len() != ids.len()
        {
            return Err("Select 1 to 16 distinct physical controllers.".into());
        }
        let at = self.journal.now();
        let sources = ids
            .iter()
            .map(|id| {
                let source = self
                    .sources
                    .get(id)
                    .filter(|s| s.ready())
                    .ok_or("Selected controller needs a complete current baseline.")?;
                let mut baseline = source.event(Reason::Baseline);
                baseline.captured_us = at;
                baseline.cursor = self.journal.cursor();
                Ok(SourceSnapshot {
                    id: id.clone(),
                    controls: source.controls.clone(),
                    baseline,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(Snapshot {
            session: self.session.clone(),
            cursor: self.journal.cursor(),
            captured_us: at,
            sources,
        })
    }
    pub fn stream_page(&self, session: &str, after: u64) -> Page {
        self.journal.page(&self.session, session, after)
    }
    pub fn stream_page_for_watch(&self, session: &str, after: u64, watch: &crate::stream::ReportWatch) -> Page {
        self.journal.page_for_watch(&self.session, session, after, watch)
    }
}

use nauron_contracts::MirEvent;

pub struct EventRecorder<'a> {
    events: Vec<MirEvent>,
    publish: &'a mut dyn FnMut(MirEvent),
}

impl<'a> EventRecorder<'a> {
    pub fn new(publish: &'a mut dyn FnMut(MirEvent)) -> Self {
        Self {
            events: Vec::new(),
            publish,
        }
    }

    pub fn push(&mut self, event: MirEvent) {
        self.events.push(event.clone());
        (self.publish)(event);
    }

    pub fn into_events(self) -> Vec<MirEvent> {
        self.events
    }
}

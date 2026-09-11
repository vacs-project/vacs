use crate::data_feed::DataFeed;
use crate::{ControllerInfo, FacilityType};
use async_trait::async_trait;
use std::sync::Mutex;
use vacs_protocol::vatsim::ClientId;

#[derive(Debug)]
pub struct MockDataFeed {
    state: Mutex<State>,
}

#[derive(Debug, Clone)]
struct State {
    should_error: bool,
    controllers: Vec<ControllerInfo>,
}

impl MockDataFeed {
    pub fn new(controllers: Vec<ControllerInfo>) -> Self {
        Self {
            state: Mutex::new(State {
                should_error: false,
                controllers,
            }),
        }
    }

    pub fn add(&self, controller: ControllerInfo) {
        let mut state = self.state.lock().unwrap();
        state.controllers.push(controller);
    }

    pub fn remove(&self, cid: &ClientId) {
        let mut state = self.state.lock().unwrap();
        state.controllers.retain(|c| c.cid != *cid);
    }

    pub fn clear(&self) {
        let mut state = self.state.lock().unwrap();
        state.controllers.clear();
    }

    pub fn set_controllers(&self, controllers: Vec<ControllerInfo>) {
        let mut state = self.state.lock().unwrap();
        state.controllers = controllers;
    }

    pub fn set_error(&self, should_error: bool) {
        let mut state = self.state.lock().unwrap();
        state.should_error = should_error;
    }
}

impl Default for MockDataFeed {
    fn default() -> Self {
        Self::new(vec![ControllerInfo {
            cid: ClientId::from("client1"),
            callsign: "client1".to_string(),
            frequency: "100.000".to_string(),
            facility_type: FacilityType::Enroute,
            visual_range: None,
        }])
    }
}

#[async_trait]
impl DataFeed for MockDataFeed {
    async fn fetch_controller_info(&self) -> crate::Result<Vec<ControllerInfo>> {
        let state = self.state.lock().unwrap();
        if state.should_error {
            return Err(crate::Error::Other("Mock error".to_string()));
        }
        Ok(state.controllers.clone())
    }
}

use std::collections::BTreeMap;
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Location {
    pub country: String,
    pub city: String,
}
#[derive(Serialize, Deserialize, Debug, Clone)]

pub struct RelayList {
    pub locations: BTreeMap<String, Location>,
    pub wireguard: WireguardList,
}
#[derive(Serialize, Deserialize, Debug, Clone)]

pub struct WireguardList {
    pub relays: Vec<Relay>
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Relay {
    pub hostname: String,
    pub location: String,
}
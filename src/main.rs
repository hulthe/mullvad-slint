// Prevent console window in addition to Slint window in Windows release builds when, e.g., starting the app via file manager. Ignored on other platforms.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

pub mod api;
mod tray;

mod my_slint {
    slint::include_modules!();

    impl Eq for Relay {}
}

use std::{
    collections::BTreeMap,
    rc::Rc,
    sync::{Arc, LazyLock},
};

use anyhow::Context;
use futures::StreamExt as _;
use mullvad_management_interface::{MullvadProxyClient, client::DaemonEvent};
use mullvad_types::states::TunnelState;
use my_slint::Country;
use slint::{ComponentHandle as _, ModelRc, VecModel, invoke_from_event_loop};
use tokio::sync::{Mutex, OwnedMappedMutexGuard, OwnedMutexGuard};

use crate::{my_slint::ConnectionState, tray::create_tray_icon};

// Convert API relay list from Rust to a Slint list of countries.
// A [ModelRc] is just a Slint list.
fn relay_list_to_slint(relay_list: &api::RelayList) -> ModelRc<Country> {
    // Transform the relay list into a tree of Country/City/Relay.
    let mut countries: BTreeMap<String, BTreeMap<String, Vec<my_slint::Relay>>> = BTreeMap::new();
    for relay in &relay_list.wireguard.relays {
        let Some(location) = relay_list.locations.get(&relay.location) else {
            continue;
        };

        let country = countries.entry(location.country.clone()).or_default();
        let city = country.entry(location.city.clone()).or_default();
        city.push(my_slint::Relay {
            hostname: relay.hostname.clone().into(),
        });
    }

    // Massage the rust BTreeMaps into a slint lists.
    // Slint does not support Maps AFAICT.
    let countries = countries
        .into_iter()
        .map(|(name, cities)| {
            let cities = cities
                .into_iter()
                .map(|(name, relays)| my_slint::City {
                    name: name.into(),
                    relays: relays.as_slice().into(),
                })
                .collect::<VecModel<my_slint::City>>();

            my_slint::Country {
                name: name.into(),
                cities: ModelRc::from(Rc::new(cities)),
            }
        })
        .collect::<VecModel<Country>>();

    ModelRc::from(Rc::new(countries))
}

const RELAY_LIST: &str = include_str!("relays.json");

#[derive(Clone)]
struct RpcTask {
    rpc: Arc<Mutex<Option<MullvadProxyClient>>>,
}

impl RpcTask {
    pub fn new() -> Self {
        Self {
            rpc: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn with_rpc<Fn, Fut, T>(&self, f: Fn) -> anyhow::Result<T>
    where
        Fn: FnOnce(OwnedMappedMutexGuard<Option<MullvadProxyClient>, MullvadProxyClient>) -> Fut,
        Fut: Future<Output = anyhow::Result<T>>,
    {
        let mut rpc_option = self.rpc.clone().lock_owned().await;

        if rpc_option.is_none() {
            let rpc = MullvadProxyClient::new()
                .await
                .context("Failed to open RPC connection")?;
            *rpc_option = Some(rpc);
        };

        let rpc = OwnedMutexGuard::map(rpc_option, |option: &mut Option<_>| {
            option.as_mut().unwrap()
        });

        f(rpc).await
    }

    pub fn invoke<Fn, Fut>(&self, f: Fn)
    where
        Fn: FnOnce(OwnedMappedMutexGuard<Option<MullvadProxyClient>, MullvadProxyClient>) -> Fut,
        Fut: Future<Output = anyhow::Result<()>>,
        Fn: Send + 'static,
        Fut: Send,
    {
        let this = self.clone();

        RT.spawn(async move {
            let result = this.with_rpc(f).await;
            if let Err(e) = result {
                eprintln!("{e:#?}");
            }
        });
    }
}

static RT: LazyLock<tokio::runtime::Runtime> = LazyLock::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
});

fn main() -> anyhow::Result<()> {
    let relay_list: api::RelayList =
        serde_json::from_str(RELAY_LIST).expect("Failed to parse relay-list");

    let rpc = RpcTask::new();
    let rpc2 = rpc.clone();

    let _tray = create_tray_icon();

    let app = my_slint::AppWindow::new()?;

    let countries = relay_list_to_slint(&relay_list);

    app.global::<my_slint::RelayList>()
        .set_countries(countries.clone());
    app.set_countries(countries);

    {
        let rpc = rpc.clone();
        app.on_connect_clicked(move || {
            rpc.invoke(|mut rpc| async move {
                if rpc.get_tunnel_state().await?.is_disconnected() {
                    rpc.connect_tunnel().await?;
                } else {
                    rpc.disconnect_tunnel().await?;
                }
                Ok(())
            });
        });
    }

    let settings = app.global::<my_slint::Settings>();

    macro_rules! bind_boolean_rpc {
        ($ui_callback:ident, $rpc_fn:ident) => {{
            let rpc = rpc.clone();
            settings.$ui_callback(move |enabled| {
                rpc.invoke(async move |mut rpc| {
                    rpc.$rpc_fn(enabled).await?;
                    Ok(())
                });
            });
        }};
    }

    bind_boolean_rpc!(on_set_allow_lan, set_allow_lan);
    bind_boolean_rpc!(on_set_enable_ipv6, set_enable_ipv6);
    bind_boolean_rpc!(on_set_daita_enabled, set_enable_daita);
    bind_boolean_rpc!(on_set_daita_direct_only, set_daita_direct_only);

    let app_weak = app.as_weak();

    RT.spawn(async move {
        let rpc = rpc2;
        let mut rpc = rpc.with_rpc(async |rpc| Ok(rpc.clone())).await.unwrap();

        let mut events = rpc
            .events_listen()
            .await
            .context("Failed to listen to events")
            .unwrap();
        let mut tunnel_state = rpc
            .get_tunnel_state()
            .await
            .context("Failed to query tunnel state")
            .unwrap();
        let mut settings = rpc
            .get_settings()
            .await
            .context("Failed to query tunnel state")
            .unwrap();

        let update_state = |tunnel_state: &TunnelState| {
            let state = match tunnel_state {
                TunnelState::Disconnected { .. } => ConnectionState::Disconnected,
                TunnelState::Connecting { .. } => ConnectionState::Connecting,
                TunnelState::Connected { .. } => ConnectionState::Connected,
                TunnelState::Disconnecting(..) => ConnectionState::Disconnecting,
                TunnelState::Error(..) => ConnectionState::Error,
            };

            let app_weak = app_weak.clone();
            invoke_from_event_loop(move || {
                if let Some(app) = app_weak.upgrade() {
                    app.global::<my_slint::Connection>().set_state(state);
                };
            })
        };

        let update_settings = |settings: &mullvad_types::settings::Settings| {
            let settings = settings.clone();
            let app_weak = app_weak.clone();
            invoke_from_event_loop(move || {
                if let Some(app) = app_weak.upgrade() {
                    let settings_ui = app.global::<my_slint::Settings>();
                    settings_ui.set_allow_lan(settings.allow_lan);
                    settings_ui.set_enable_ipv6(settings.tunnel_options.generic.enable_ipv6);
                    settings_ui.set_daita_enabled(settings.tunnel_options.wireguard.daita.enabled);
                    settings_ui.set_daita_direct_only(
                        !settings
                            .tunnel_options
                            .wireguard
                            .daita
                            .use_multihop_if_necessary,
                    );
                };
            })
        };

        update_state(&tunnel_state).unwrap();
        update_settings(&settings).unwrap();

        loop {
            let event = events.next().await;
            let Some(Ok(event)) = event else { break };
            match event {
                DaemonEvent::TunnelState(new) => {
                    tunnel_state = new;
                    if update_state(&tunnel_state).is_err() {
                        break;
                    }
                }
                DaemonEvent::Settings(new) => {
                    settings = new;
                    if update_settings(&settings).is_err() {
                        break;
                    }
                }
                _ => continue,
            }
        }
    });

    app.run()?;

    Ok(())
}

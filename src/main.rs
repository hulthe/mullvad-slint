// Prevent console window in addition to Slint window in Windows release builds when, e.g., starting the app via file manager. Ignored on other platforms.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

pub mod api;
mod rpc;

#[cfg(feature = "tray-icon")]
mod tray;

mod my_slint {
    slint::include_modules!();

    impl Eq for Relay {}
}

use std::{rc::Rc, sync::LazyLock};

use anyhow::{Context, bail};
use futures::StreamExt as _;
use mullvad_management_interface::client::DaemonEvent;
use mullvad_types::{
    constraints::Constraint,
    relay_constraints::{GeographicLocationConstraint, LocationConstraint, RelaySettings},
    relay_list::RelayList,
    states::TunnelState,
};
use my_slint::Country;
use slint::{ComponentHandle as _, ModelRc, ToSharedString, VecModel};

use crate::{my_slint::ConnectionState, rpc::Rpc};

/// Convert gRPC relay list from Rust to a Slint list of countries.
fn relay_list_to_slint(relay_list: &RelayList) -> ModelRc<Country> {
    let countries = relay_list
        .countries
        .iter()
        .map(|country| {
            let cities = country
                .cities
                .iter()
                .map(|city| {
                    let relays = city
                        .relays
                        .iter()
                        .map(|relay| my_slint::Relay {
                            hostname: relay.hostname.to_shared_string(),
                        })
                        .collect::<VecModel<_>>();
                    my_slint::City {
                        name: city.name.to_shared_string(),
                        code: city.code.to_shared_string(),
                        relays: ModelRc::from(Rc::new(relays)),
                    }
                })
                .collect::<VecModel<_>>();

            my_slint::Country {
                name: country.name.to_shared_string(),
                code: country.code.to_shared_string(),
                cities: ModelRc::from(Rc::new(cities)),
            }
        })
        .collect::<VecModel<_>>();

    ModelRc::from(Rc::new(countries))
}

static RT: LazyLock<tokio::runtime::Runtime> = LazyLock::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("Failed to create tokio runtime")
});

fn main() -> anyhow::Result<()> {
    let rpc = Rpc::new();

    #[cfg(feature = "tray-icon")]
    let _tray = tray::create_tray_icon();

    let app = my_slint::AppWindow::new()?;

    let ui_state = app.global::<my_slint::State>();

    {
        let rpc = rpc.clone();
        ui_state.on_connect_button(move || {
            rpc.spawn_with_rpc(|mut rpc| async move {
                if rpc.get_tunnel_state().await?.is_disconnected() {
                    rpc.connect_tunnel().await?;
                } else {
                    rpc.disconnect_tunnel().await?;
                }
                Ok(())
            });
        });
    }

    {
        let rpc = rpc.clone();
        ui_state.on_select_country(move |country| {
            rpc.spawn_with_rpc(|mut rpc| async move {
                let relay_settings = rpc.get_settings().await?.relay_settings;
                let RelaySettings::Normal(mut relay_constraints) = relay_settings else {
                    bail!("Can't configure custom relays");
                };
                relay_constraints.location = Constraint::Only(LocationConstraint::Location(
                    GeographicLocationConstraint::Country(country.code.into()),
                ));
                rpc.set_relay_settings(RelaySettings::Normal(relay_constraints))
                    .await?;

                Ok(())
            });
        });
    }

    {
        let rpc = rpc.clone();
        ui_state.on_select_city(move |country, city| {
            rpc.spawn_with_rpc(|mut rpc| async move {
                let relay_settings = rpc.get_settings().await?.relay_settings;
                let RelaySettings::Normal(mut relay_constraints) = relay_settings else {
                    bail!("Can't configure custom relays");
                };
                relay_constraints.location = Constraint::Only(LocationConstraint::Location(
                    GeographicLocationConstraint::City(country.code.into(), city.code.into()),
                ));
                rpc.set_relay_settings(RelaySettings::Normal(relay_constraints))
                    .await?;
                Ok(())
            });
        });
    }

    {
        let rpc = rpc.clone();
        ui_state.on_select_relay(move |country, city, relay| {
            rpc.spawn_with_rpc(|mut rpc| async move {
                let relay_settings = rpc.get_settings().await?.relay_settings;
                let RelaySettings::Normal(mut relay_constraints) = relay_settings else {
                    bail!("Can't configure custom relays");
                };
                relay_constraints.location = Constraint::Only(LocationConstraint::Location(
                    GeographicLocationConstraint::Hostname(
                        country.code.into(),
                        city.code.into(),
                        relay.hostname.into(),
                    ),
                ));
                rpc.set_relay_settings(RelaySettings::Normal(relay_constraints))
                    .await?;
                Ok(())
            });
        });
    }

    macro_rules! bind_boolean_rpc {
        ($ui_callback:ident, $rpc_fn:ident) => {{
            let rpc = rpc.clone();
            ui_state.$ui_callback(move |enabled| {
                rpc.spawn_with_rpc(async move |mut rpc| {
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

    // Populate relay list
    let app_weak = app.as_weak();
    rpc.spawn_with_rpc(async move |mut rpc| {
        let relay_list = rpc
            .get_relay_locations()
            .await
            .context("Failed to get relay list")?;
        app_weak.upgrade_in_event_loop(move |app| {
            let countries = relay_list_to_slint(&relay_list);
            app.global::<my_slint::RelayList>().set_countries(countries);
        })?;

        anyhow::Ok(())
    });

    // Listen for events
    let app_weak = app.as_weak();
    rpc.spawn_with_rpc(async move |mut rpc| {
        let mut events = rpc
            .events_listen()
            .await
            .context("Failed to listen to events")?;
        let mut tunnel_state = rpc
            .get_tunnel_state()
            .await
            .context("Failed to query tunnel state")?;
        let mut settings = rpc
            .get_settings()
            .await
            .context("Failed to query tunnel state")?;

        let update_state = |tunnel_state: &TunnelState| {
            let location = tunnel_state.get_location();
            let conn_state = match tunnel_state {
                TunnelState::Disconnected { .. } => ConnectionState::Disconnected,
                TunnelState::Connecting { .. } => ConnectionState::Connecting,
                TunnelState::Connected { .. } => ConnectionState::Connected,
                TunnelState::Disconnecting(..) => ConnectionState::Disconnecting,
                TunnelState::Error(..) => ConnectionState::Error,
            };

            let hostname = location
                .and_then(|l| l.hostname.as_deref())
                .unwrap_or_default()
                .to_shared_string();

            let country = location.map(|l| l.country.as_str()).unwrap_or_default();
            let city = location.and_then(|l| l.city.as_deref());

            let location = if let Some(city) = city {
                format!("{country}, {city}").to_shared_string()
            } else {
                country.to_shared_string()
            };

            app_weak.upgrade_in_event_loop(move |app| {
                let state = app.global::<my_slint::State>();
                state.set_conn(conn_state);
                state.set_location(location);
                state.set_relay_hostname(hostname);
            })
        };

        let update_selected_relay = |ui_state: &my_slint::State, relay_settings: &RelaySettings| {
            let mut country = "";
            let mut city = "";
            let mut relay = "";

            loop {
                let RelaySettings::Normal(relay_constraints) = relay_settings else {
                    break;
                };

                let Constraint::Only(location) = &relay_constraints.location else {
                    break;
                };

                let LocationConstraint::Location(location) = location else {
                    break; // TODO: custom list
                };

                match location {
                    GeographicLocationConstraint::Country(country_code) => {
                        country = &country_code;
                    }
                    GeographicLocationConstraint::City(country_code, city_code) => {
                        country = &country_code;
                        city = &city_code;
                    }
                    GeographicLocationConstraint::Hostname(country_code, city_code, hostname) => {
                        country = &country_code;
                        city = &city_code;
                        relay = &hostname;
                    }
                }

                break;
            }

            ui_state.set_selected_country(country.into());
            ui_state.set_selected_city(city.into());
            ui_state.set_selected_relay(relay.into());
        };

        let update_settings = |settings: &mullvad_types::settings::Settings| {
            let settings = settings.clone();
            app_weak.upgrade_in_event_loop(move |app| {
                let ui_state = app.global::<my_slint::State>();

                update_selected_relay(&ui_state, &settings.relay_settings);
                ui_state.set_allow_lan(settings.allow_lan);
                ui_state.set_enable_ipv6(settings.tunnel_options.generic.enable_ipv6);
                ui_state.set_daita_enabled(settings.tunnel_options.wireguard.daita.enabled);
                ui_state.set_daita_direct_only(
                    !settings
                        .tunnel_options
                        .wireguard
                        .daita
                        .use_multihop_if_necessary,
                );
            })
        };

        update_state(&tunnel_state)?;
        update_settings(&settings)?;

        while let Some(event) = events.next().await {
            match event? {
                DaemonEvent::TunnelState(new) => {
                    tunnel_state = new;
                    update_state(&tunnel_state)?;
                }
                DaemonEvent::Settings(new) => {
                    settings = new;
                    update_settings(&settings)?;
                }
                _ => continue,
            }
        }

        Ok(())
    });

    app.run()?;

    Ok(())
}

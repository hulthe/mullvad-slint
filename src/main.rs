// Prevent console window in addition to Slint window in Windows release builds when, e.g., starting the app via file manager. Ignored on other platforms.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

pub mod api;
#[cfg(feature = "map")]
mod map;
mod rpc;

#[cfg(target_os = "linux")]
mod split_tunneling;

#[cfg(all(target_os = "linux", feature = "tray-icon"))]
mod tray;

mod slint_ty;

use std::{rc::Rc, sync::LazyLock};

use anyhow::{Context, bail};
use clap::Parser;
use futures::StreamExt as _;
use mullvad_management_interface::client::DaemonEvent;
use mullvad_types::{
    constraints::Constraint,
    relay_constraints::{GeographicLocationConstraint, LocationConstraint, RelaySettings},
    relay_list::RelayList,
    states::TunnelState,
};
#[cfg(feature = "map")]
use slint::wgpu_28::{WGPUConfiguration, WGPUSettings};
use slint::{ComponentHandle as _, Model, ModelRc, ToSharedString, VecModel};
use slint_ty::Country;

use crate::{
    rpc::Rpc,
    slint_ty::{ConnectionState, Route, View},
};

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
                        .map(|relay| slint_ty::Relay {
                            hostname: relay.hostname.to_shared_string(),
                        })
                        .collect::<VecModel<_>>();
                    slint_ty::City {
                        name: city.name.to_shared_string(),
                        code: city.code.to_shared_string(),
                        relays: ModelRc::from(Rc::new(relays)),
                        latitude: city.latitude as f32,
                        longitude: city.longitude as f32,
                    }
                })
                .collect::<VecModel<_>>();

            slint_ty::Country {
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

#[derive(Parser)]
struct Opt {
    #[clap(long, env = "RUST_LOG", default_value = "info")]
    log_filter: String,
}

fn main() -> anyhow::Result<()> {
    let opt = Opt::parse();

    let fmt_subscriber = tracing_subscriber::FmtSubscriber::builder()
        .with_env_filter(&opt.log_filter)
        .finish();
    tracing::subscriber::set_global_default(fmt_subscriber)
        .context("Failed to initialize tracing subscriber")?;

    // Set up wgpu backend if map feature is enabled
    #[cfg(feature = "map")]
    {
        let wgpu_settings = WGPUSettings::default();
        slint::BackendSelector::new()
            .require_wgpu_28(WGPUConfiguration::Automatic(wgpu_settings))
            .select()
            .expect("Unable to create Slint backend with WGPU based renderer");
    }

    let rpc = Rpc::new();

    #[cfg(all(target_os = "linux", feature = "tray-icon"))]
    let _tray = tray::create_tray_icon();

    let app = slint_ty::AppWindow::new()?;

    let ui_state = app.global::<slint_ty::State>();

    {
        // Install connect button callback
        let rpc = rpc.clone();
        ui_state.on_connect_button(move || {
            rpc.spawn_with_rpc(async move |mut rpc| {
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
        // Install select country callback
        let rpc = rpc.clone();
        ui_state.on_select_country(move |country| {
            rpc.spawn_with_rpc(async move |mut rpc| {
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
        // Install select city callback
        let rpc = rpc.clone();
        ui_state.on_select_city(move |country, city| {
            rpc.spawn_with_rpc(async move |mut rpc| {
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
        // Install select relay callback
        let rpc = rpc.clone();
        ui_state.on_select_relay(move |country, city, relay| {
            rpc.spawn_with_rpc(async move |mut rpc| {
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

    {
        // Install device ip version callback
        let rpc = rpc.clone();
        ui_state.on_set_device_ip_version(move |device_ip_version| {
            rpc.spawn_with_rpc(async move |mut rpc| {
                let relay_settings = rpc.get_settings().await?.relay_settings;
                let RelaySettings::Normal(mut relay_constraints) = relay_settings else {
                    bail!("Can't configure custom relays");
                };
                relay_constraints.wireguard_constraints.ip_version = device_ip_version.into();
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
            app.global::<slint_ty::RelayList>().set_countries(countries);
        })?;

        anyhow::Ok(())
    });

    // Install location search callback
    let app_weak = app.as_weak();
    app.global::<slint_ty::RelayList>()
        .on_search_location(move |search| {
            let search = search.to_lowercase();
            let _ = app_weak.upgrade_in_event_loop(move |app| {
                let relay_list = app.global::<slint_ty::RelayList>();
                let countries = relay_list.get_countries();
                let filtered_countries: VecModel<_> = countries
                    .iter()
                    .filter(|meta| meta.name.to_lowercase().contains(search.as_str()))
                    .collect();
                relay_list.set_filtered_countries(ModelRc::new(Rc::new(filtered_countries)));
            });
        });

    // Listen for events
    async fn listen_for_events(
        mut rpc: mullvad_management_interface::MullvadProxyClient,
        app_weak: slint::Weak<slint_ty::AppWindow>,
    ) -> anyhow::Result<()> {
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

        let mut last_latlong = (0.0, 0.0);
        let mut update_state = |tunnel_state: &TunnelState| {
            let location = tunnel_state.get_location();
            let conn_state = ConnectionState::from(tunnel_state);

            let hostname = location
                .and_then(|l| l.hostname.as_deref())
                .unwrap_or_default()
                .to_shared_string();

            let country = location.map(|l| l.country.as_str()).unwrap_or_default();
            let city = location.and_then(|l| l.city.as_deref());

            let latlong = location
                .map(|l| (l.latitude as f32, l.longitude as f32))
                .filter(|&new| {
                    if new != last_latlong {
                        last_latlong = new;
                        true
                    } else {
                        false
                    }
                });

            let location = if let Some(city) = city {
                format!("{country}, {city}").to_shared_string()
            } else {
                country.to_shared_string()
            };

            app_weak.upgrade_in_event_loop(move |app| {
                if let Some((latitude, longitude)) = latlong {
                    app.set_latitude(latitude);
                    app.set_longitude(longitude);
                }
                let state = app.global::<slint_ty::State>();
                state.set_conn(conn_state);
                state.set_location(location);
                state.set_relay_hostname(hostname);
            })
        };

        let update_relay_settings = |ui_state: &slint_ty::State, relay_settings: &RelaySettings| {
            let mut device_ip_version = slint_ty::DeviceIpVersion::Auto;
            let mut country = "";
            let mut city = "";
            let mut relay = "";

            (|| {
                let RelaySettings::Normal(relay_constraints) = relay_settings else {
                    return;
                };

                device_ip_version = relay_constraints.wireguard_constraints.ip_version.into();

                let Constraint::Only(location) = &relay_constraints.location else {
                    return;
                };

                let LocationConstraint::Location(location) = location else {
                    return; // TODO: custom list
                };

                match location {
                    GeographicLocationConstraint::Country(country_code) => {
                        country = country_code;
                    }
                    GeographicLocationConstraint::City(country_code, city_code) => {
                        country = country_code;
                        city = city_code;
                    }
                    GeographicLocationConstraint::Hostname(country_code, city_code, hostname) => {
                        country = country_code;
                        city = city_code;
                        relay = hostname;
                    }
                }
            })();

            ui_state.set_device_ip_version(device_ip_version);
            ui_state.set_selected_country(country.into());
            ui_state.set_selected_city(city.into());
            ui_state.set_selected_relay(relay.into());
        };

        let update_settings = |settings: &mullvad_types::settings::Settings| {
            let settings = settings.clone();
            app_weak.upgrade_in_event_loop(move |app| {
                let ui_state = app.global::<slint_ty::State>();

                update_relay_settings(&ui_state, &settings.relay_settings);
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

        let _ = app_weak.upgrade_in_event_loop(|app| {
            app.global::<Route>()
                .set_connecting_to_service(View { show: false });
        });

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
    }

    let app_weak = app.as_weak();
    rpc.spawn_with_rpc_retry_on_error(async move |rpc| {
        listen_for_events(rpc, app_weak.clone())
            .await
            .inspect_err(|_| {
                let _ = app_weak.upgrade_in_event_loop(|app| {
                    app.global::<Route>()
                        .set_connecting_to_service(View { show: true });
                });
            })
    });

    // Populate app list
    #[cfg(target_os = "linux")]
    split_tunneling::setup(&app);

    #[cfg(feature = "map")]
    {
        let app_weak = app.as_weak();
        let mut map_renderer: Option<map::Map> = None;

        app.window()
            .set_rendering_notifier(move |state, graphics_api| {
                match state {
                    slint::RenderingState::RenderingSetup => {
                        // Initialize the map renderer when we have access to the wgpu device
                        if let slint::GraphicsAPI::WGPU28 { device, queue, .. } = graphics_api {
                            let app = app_weak.upgrade().unwrap();
                            let size = app.window().size();
                            map_renderer = Some(map::Map::new(device, queue, size));
                        }
                    }
                    slint::RenderingState::BeforeRendering => {
                        // Render the map before each frame
                        if let (Some(map), Some(app)) = (map_renderer.as_mut(), app_weak.upgrade())
                        {
                            let size = app.window().size();
                            let texture = map.render(map::MapInput {
                                size,
                                coords: glam::Vec2::new(app.get_latitude(), app.get_longitude()),
                                zoom: app.get_zoom(),
                            });

                            if let Some(texture) = texture {
                                if let Ok(image) = slint::Image::try_from(texture) {
                                    app.set_map(image);
                                }
                            }
                            app.window().request_redraw();
                        }
                    }
                    slint::RenderingState::RenderingTeardown => {
                        // Clean up the map renderer
                        drop(map_renderer.take());
                    }
                    _ => {}
                }
            })
            .expect("Failed to set up rendering notifier");
    }

    app.run()?;

    Ok(())
}

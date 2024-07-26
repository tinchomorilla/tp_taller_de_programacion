use std::collections::HashMap;

use crate::apps::apps_mqtt_topics::AppsMqttTopics;
use crate::apps::incident_data::{incident::Incident, incident_info::IncidentInfo, incident_source::IncidentSource};
use crate::apps::place_type::PlaceType;
use crate::apps::sist_camaras::camera_state::CameraState;
use crate::apps::sist_dron::dron_current_info::DronCurrentInfo;
use crate::apps::sist_dron::dron_state::DronState;
use crate::mqtt::messages::publish_message::PublishMessage;

use crate::apps::sist_camaras::camera::Camera;
use crate::apps::vendor::{
    HttpOptions, Map, MapMemory, Place, Places, Position, Style, Tiles, TilesManager,
};
use crate::apps::{places, plugins::ImagesPluginData};
use crossbeam::channel::Receiver;
use egui::Context;
use egui::{menu, Color32};
use std::sync::mpsc::Sender;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Provider {
    OpenStreetMap,
    Geoportal,
    MapboxStreets,
    MapboxSatellite,
    LocalTiles,
}

fn http_options() -> HttpOptions {
    HttpOptions {
        cache: None,
        /*cache: if std::env::var("NO_HTTP_CACHE").is_ok() {
            None
        } else {
            Some(".cache".into())
        },*/
        ..Default::default()
    }
}

fn providers(egui_ctx: Context) -> HashMap<Provider, Box<dyn TilesManager + Send>> {
    let mut providers: HashMap<Provider, Box<dyn TilesManager + Send>> = HashMap::default();

    providers.insert(
        Provider::OpenStreetMap,
        Box::new(Tiles::with_options(
            super::super::vendor::sources::OpenStreetMap,
            http_options(),
            egui_ctx.to_owned(),
        )),
    );

    providers.insert(
        Provider::Geoportal,
        Box::new(Tiles::with_options(
            super::super::vendor::sources::Geoportal,
            http_options(),
            egui_ctx.to_owned(),
        )),
    );

    providers.insert(
        Provider::LocalTiles,
        Box::new(super::super::local_tiles::LocalTiles::new(
            egui_ctx.to_owned(),
        )),
    );

    // Pass in a mapbox access token at compile time. May or may not be what you want to do,
    // potentially loading it from application settings instead.
    let mapbox_access_token = std::option_env!("MAPBOX_ACCESS_TOKEN");

    // We only show the mapbox map if we have an access token
    if let Some(token) = mapbox_access_token {
        providers.insert(
            Provider::MapboxStreets,
            Box::new(Tiles::with_options(
                super::super::vendor::sources::Mapbox {
                    style: super::super::vendor::sources::MapboxStyle::Streets,
                    access_token: token.to_string(),
                    high_resolution: false,
                },
                http_options(),
                egui_ctx.to_owned(),
            )),
        );
        providers.insert(
            Provider::MapboxSatellite,
            Box::new(Tiles::with_options(
                super::super::vendor::sources::Mapbox {
                    style: super::super::vendor::sources::MapboxStyle::Satellite,
                    access_token: token.to_string(),
                    high_resolution: true,
                },
                http_options(),
                egui_ctx.to_owned(),
            )),
        );
    }

    providers
}

#[derive(Debug)]
struct IncidentWithDrones {
    incident_info: IncidentInfo,
    drones: Vec<DronCurrentInfo>,
}

pub struct UISistemaMonitoreo {
    providers: HashMap<Provider, Box<dyn TilesManager + Send>>,
    selected_provider: Provider,
    map_memory: MapMemory,
    images_plugin_data: ImagesPluginData,
    click_watcher: super::super::plugins::ClickWatcher,
    incident_dialog_open: bool,
    latitude: String,
    longitude: String,
    publish_incident_tx: Sender<Incident>,
    publish_message_rx: Receiver<PublishMessage>,
    places: Places,
    last_incident_id: u8,
    exit_tx: Sender<bool>,
    incidents_to_resolve: Vec<IncidentWithDrones>, // posicion 0  --> (inc_id_to_resolve, drones(dron1, dron2)) // posicion 1 --> (inc_id_to_resolve 2, drones(dron1, dron2))
    hashmap_incidents: HashMap<IncidentInfo, Incident>, // 
}

impl UISistemaMonitoreo {
    pub fn new(
        egui_ctx: Context,
        tx: Sender<Incident>,
        publish_message_rx: Receiver<PublishMessage>,
        exit_tx: Sender<bool>,
    ) -> Self {
        egui_extras::install_image_loaders(&egui_ctx);

        // Data for the `images` plugin showcase.
        let images_plugin_data = ImagesPluginData::new(egui_ctx.to_owned());

        let mantainance_style = Style {
            symbol_color: Color32::from_rgb(255, 165, 0), // Color naranja
            ..Default::default()
        };

        let mantainance_ui = Place {
            position: places::mantenimiento(),
            label: "Mantenimiento".to_string(),
            symbol: '🔋',
            style: mantainance_style, //ESTE ES DEL LABEL, NO DEL ICONO
            id: 0,
            place_type: PlaceType::Mantainance,
        };

        let mut places = Places::new();
        places.add_place(mantainance_ui);

        Self {
            providers: providers(egui_ctx.to_owned()),
            selected_provider: Provider::OpenStreetMap,
            map_memory: MapMemory::default(),
            images_plugin_data,
            click_watcher: Default::default(),
            incident_dialog_open: false,
            latitude: String::new(),
            longitude: String::new(),
            publish_incident_tx: tx,
            publish_message_rx,
            places,
            last_incident_id: 0,
            exit_tx,
            incidents_to_resolve: Vec::new(),
            hashmap_incidents: HashMap::new(),
        }
    }
    
    /// Envía internamente a otro hilo el `incident` recibido, para publicarlo por mqtt.
    fn send_incident_for_publish(&self, incident: Incident) {
        println!("Enviando incidente: {:?}", incident);
        let _ = self.publish_incident_tx.send(incident);
    }
    /// Se encarga de procesar y agregar o eliminar una cámara recibida al mapa.
    fn handle_camera_message(&mut self, publish_message: PublishMessage) {
        let camera = Camera::from_bytes(&publish_message.get_payload());
        println!(
            "UI: recibida cámara: {:?}, estado: {:?}",
            camera,
            camera.get_state()
        );

        if camera.is_not_deleted() {
            let camera_id = camera.get_id();
            let (latitude, longitude) = (camera.get_latitude(), camera.get_longitude());
            // Si existía, la elimino del mapa, para volver a dibujarla (xq puede tener cambiado el estado)
            self.places.remove_place(camera_id, PlaceType::Camera);

            // Se le pone un color dependiendo de su estado
            let style = match camera.get_state() {
                CameraState::Active => Style {
                    symbol_color: Color32::from_rgb(0, 255, 0), // Color verde
                    ..Default::default()
                },
                CameraState::SavingMode => Style::default(),
            };

            let camera_ui = Place {
                position: Position::from_lon_lat(longitude, latitude),
                label: format!("Camera {}", camera_id),
                symbol: '📷',
                style, //ESTE ES DEL LABEL, NO DEL ICONO
                id: camera_id,
                place_type: PlaceType::Camera,
            };
            self.places.add_place(camera_ui);
        } else {
            self.places
                .remove_place(camera.get_id(), PlaceType::Camera);
        }
    }

    /// Se encarga de procesar y agregar un dron recibido al mapa.
    fn handle_drone_message(&mut self, msg: PublishMessage) {
        if let Ok(dron) = DronCurrentInfo::from_bytes(msg.get_payload()) {
            /*println!(
                "UI: recibido dron: {:?}, estado: {:?}",
                dron,
                dron.get_state()
            );*/
            // Si ya existía el dron, se lo elimina, porque que me llegue nuevamente significa que se está moviendo.
            let dron_id = dron.get_id();
            self.places.remove_place(dron_id, PlaceType::Dron);

            if dron.get_state() == DronState::ManagingIncident {
                // Llegó a la posición del inc.
                if let Some(inc_info) = dron.get_inc_id_to_resolve() {
                    // Busca el incidente en el vector.
                    let incident_index = self
                        .incidents_to_resolve
                        .iter()
                        .position(|incident| incident.incident_info == inc_info);
                        //.position(|incident| incident.incident_info.get_inc_id() == inc_id); // <--pre refactor decía esto

                    match incident_index {
                        Some(index) => {
                            // Si el incidente ya existe, agrega el dron al vector de drones del incidente.
                            self.incidents_to_resolve[index].drones.push(dron.clone());
                        }
                        None => {
                            // Aux para que compile, temporalmente:
                            // aux: let inc_info = IncidentInfo::new(inc_id, IncidentSource::Manual);
                            // Si no tengo guardado el inc_id_to_res, crea una nueva posicion con el dron respectivo.
                            self.incidents_to_resolve.push(IncidentWithDrones {
                                incident_info: inc_info,
                                drones: vec![dron.clone()],
                            });
                        }
                    }
                }
            }

            //posicion 0  --> (inc_id_to_resolve = 1, drones(dron1, dron2))

            /*println!(
                "EL vector de incidentes a resolver es: {:?}",
                self.incidents_to_resolve
            );*/

            for incident in self.incidents_to_resolve.iter() {
                if incident.drones.len() == 2 {
                    let inc_info = &incident.incident_info;
                    if let Some(mut incident) = self.hashmap_incidents.remove(inc_info) {
                        incident.set_resolved();
                        // Obtengo el source del incidente, para pasarle un place_type acorde al remove_place
                        // y lo remuevo de la lista de places a mostrar en el mapa.
                        let place_type = PlaceType::from_inc_source(incident.get_source());                        
                        self.places.remove_place(inc_info.get_inc_id(), place_type);
                        
                        self.send_incident_for_publish(incident);
                    }
                }
            }

            let (lat, lon) = dron.get_current_position();
            let dron_pos = Position::from_lon_lat(lon, lat);

            // Se crea el label a mostrar por pantalla, según si está o no volando.
            let dron_label;
            if let Some((dir, speed)) = dron.get_flying_info() {
                let (dir_lat, dir_lon) = dir;
                // El dron está volando.
                dron_label = format!(
                    "Dron {}\n   dir: ({:.2}, {:.2})\n   vel: {} km/h",
                    dron_id, dir_lat, dir_lon, speed
                );
            } else {
                dron_label = format!("Dron {}", dron_id);
            }

            // Se crea el place y se lo agrega al mapa.
            let dron_ui = Place {
                position: dron_pos,
                label: dron_label,
                symbol: '🚁',
                style: Style::default(),
                id: dron.get_id(),
                place_type: PlaceType::Dron, // Para luego buscarlo en el places.
            };

            self.places.add_place(dron_ui);
        }
        //let _ = self.repaint_tx.send(true);
        //let _ = self.repaint_tx.send(true);
    }

    /// Recibe un PublishMessage de topic Inc, y procesa el incidente recibido
    /// (se lo guarda para continuar procesándolo, y lo muestra en la ui).
    fn handle_incident_message(&mut self, msg: PublishMessage) {
        println!("Recibo inc desde cámaras"); // o desde 'self'.
        if let Ok(inc) = Incident::from_bytes(msg.get_payload()){
            // Agregamos el incidente (add_incident) solamente si él no fue creado por sist monitoreo.
            if *inc.get_source() != IncidentSource::Manual {
                self.add_incident(&inc);
            }
        }

    }

    /// Crea el Place para el incidente recibido, lo agrega a la ui para que se muestre por pantalla,
    /// y lo agrega a un hashmap para continuar procesándolo (Aux: rever tema ids que quizás se pisen cuando camaras publiquen incs).
    fn add_incident(&mut self, incident: &Incident) {
        let custom_style = Style {
            symbol_color: Color32::from_rgb(255, 0, 0), // Color rojo
            ..Default::default()
        };

        let place_type = PlaceType::from_inc_source(incident.get_source());

        let (lat, lon) = incident.get_position();
        let new_place_incident = Place {
            position: Position::from_lon_lat(
                lon, lat,
            ),
            label: format!("Incident {}", incident.get_id()),
            symbol: '⚠',
            style: custom_style,
            id: incident.get_id(),
            place_type,
        };
        self.places.add_place(new_place_incident);

        let inc_info = IncidentInfo::new(incident.get_id(), *incident.get_source());
        let inc_to_store = incident.clone();
        self.hashmap_incidents
            .insert(inc_info, inc_to_store); // Edit: viendo :). Aux: cuando cámaras generen incidentes, rever esto xq pueden pisarse los ids.
    }

    fn get_next_incident_id(&mut self) -> u8 {
        self.last_incident_id += 1;
        self.last_incident_id
    }
}

impl eframe::App for UISistemaMonitoreo {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let rimless = egui::Frame {
            fill: ctx.style().visuals.panel_fill,
            ..Default::default()
        };

        egui::CentralPanel::default().show(ctx, |_ui| {
            ctx.request_repaint_after(std::time::Duration::from_millis(150));
        });

        egui::CentralPanel::default().show(ctx, |_ui| {
            if let Ok(publish_message) = self.publish_message_rx.try_recv() {
                if publish_message.get_topic_name() == AppsMqttTopics::CameraTopic.to_str() {
                    self.handle_camera_message(publish_message);
                } else if publish_message.get_topic_name() == AppsMqttTopics::DronTopic.to_str() {
                    self.handle_drone_message(publish_message);
                } else if publish_message.get_topic_name() == AppsMqttTopics::IncidentTopic.to_str() {
                    self.handle_incident_message(publish_message);
                }
            }
        });

        egui::CentralPanel::default()
            .frame(rimless)
            .show(ctx, |ui| {
                let my_position = places::obelisco();

                let tiles = self
                    .providers
                    .get_mut(&self.selected_provider)
                    .unwrap()
                    .as_mut();

                let map = Map::new(Some(tiles), &mut self.map_memory, my_position)
                    .with_plugin(self.places.clone())
                    .with_plugin(super::super::plugins::images(&mut self.images_plugin_data))
                    .with_plugin(super::super::plugins::CustomShapes {})
                    .with_plugin(&mut self.click_watcher);

                ui.add(map);

                {
                    use super::super::windows::*;
                    zoom(ui, &mut self.map_memory);
                    go_to_my_position(ui, &mut self.map_memory);
                    self.click_watcher.show_position(ui);
                    controls(
                        ui,
                        &mut self.selected_provider,
                        &mut self.providers.keys(),
                        &mut self.images_plugin_data,
                    );
                }

                egui::TopBottomPanel::top("top_menu").show(ctx, |ui| {
                    egui::menu::bar(ui, |ui| {
                        menu::bar(ui, |ui| {
                            ui.menu_button("Incidente", |ui| {
                                if !self.incident_dialog_open
                                    && ui.button("Alta Incidente").clicked()
                                {
                                    self.incident_dialog_open = true;
                                }
                                if self.incident_dialog_open {
                                    ui.add_space(5.0);
                                    ui.horizontal(|ui| {
                                        ui.label("Latitud:");
                                        let _latitude_input = ui.add_sized(
                                            [100.0, 20.0],
                                            egui::TextEdit::singleline(&mut self.latitude),
                                        );
                                        ui.label("Longitud:");
                                        let _longitude_input = ui.add_sized(
                                            [100.0, 20.0],
                                            egui::TextEdit::singleline(&mut self.longitude),
                                        );

                                        if ui.button("OK").clicked() {
                                            let latitude_text = self.latitude.to_string();
                                            let longitude_text = self.longitude.to_string();

                                            println!("Latitud: {}", latitude_text);
                                            println!("Longitud: {}", longitude_text);

                                            let latitude = latitude_text.parse::<f64>().unwrap();
                                            let longitude: f64 =
                                                longitude_text.parse::<f64>().unwrap();
                                            let location = (latitude, longitude);
                                            let incident = Incident::new(
                                                self.get_next_incident_id(),
                                                location,
                                                IncidentSource::Manual,
                                            );
                                            self.add_incident(&incident);
                                            self.send_incident_for_publish(incident); // lo publica
                                            self.incident_dialog_open = false;
                                        }
                                    });
                                }
                            });
                            if ui.button("Salir").clicked() {
                                // Indicar que se desea salir
                                match self.exit_tx.send(true) {
                                    Ok(_) => println!("Iniciando proceso para salir"),
                                    Err(_) => println!("Error al intentar salir"),
                                }
                            }
                        });
                    });
                });
            });
    }
}

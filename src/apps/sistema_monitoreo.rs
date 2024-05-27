extern crate gio;
extern crate gtk;
use gio::prelude::*;
use gtk::prelude::*;
use rustx::apps::camera::Camera;
use rustx::apps::incident::Incident;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc::RecvTimeoutError;
//use rustx::apps::camera::Camera;
use rustx::mqtt_client::MQTTClient;
//use std::env::args;
use rustx::apps::api_sistema_monitoreo::SistemaMonitoreo;
use std::error::Error;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle}; // Import the `SistemaMonitoreo` type

/// Lee el IP del cliente y el puerto en el que el cliente se va a conectar al servidor.
fn load_ip_and_port() -> Result<(String, u16), Box<dyn Error>> {
    let argv = std::env::args().collect::<Vec<String>>();
    if argv.len() != 3 {
        return Err(Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Cantidad de argumentos inválido. Debe ingresar: la dirección IP del sistema monitoreo y 
            el puerto en el que desea correr el servidor.",
        )));
    }
    let ip = &argv[1];
    let port = match argv[2].parse::<u16>() {
        Ok(port) => port,
        Err(_) => {
            return Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "El puerto proporcionado no es válido",
            )))
        }
    };

    Ok((ip.to_string(), port))
}

fn establish_mqtt_broker_connection(
    broker_addr: &SocketAddr,
) -> Result<MQTTClient, Box<dyn std::error::Error>> {
    let client_id = "Sistema-Monitoreo";
    let mqtt_client_res = MQTTClient::connect_to_broker(client_id, broker_addr);
    match mqtt_client_res {
        Ok(mqtt_client) => {
            println!("Cliente: Conectado al broker MQTT.");
            Ok(mqtt_client)
        }
        Err(e) => {
            println!("Sistema-Camara: Error al conectar al broker MQTT: {:?}", e);
            Err(e.into())
        }
    }
}

fn subscribe_to_topics(mqtt_client: Arc<Mutex<MQTTClient>>) {
    if let Ok(mut mqtt_client) = mqtt_client.lock() {
        let res_sub = mqtt_client.mqtt_subscribe(vec![(String::from("Cam"))]);
        match res_sub {
            Ok(_) => println!("Cliente: Hecho un subscribe"),
            Err(e) => println!("Cliente: Error al hacer un subscribe: {:?}", e),
        }
    }

    // Que lea del topic al/os cual/es hizo subscribe.
    loop {
        if let Ok(mqtt_client) = mqtt_client.lock() {
            match mqtt_client.mqtt_receive_msg_from_subs_topic() {
                Ok(msg) => {
                    println!("Cliente: Recibo estos msg_bytes: {:?}", msg);

                    // Ya puedo obtener la cámara recibida
                    let camera_recibida = Camera::from_bytes(&msg.get_payload());
                    println!("Cliente: Recibo cámara: {:?}", camera_recibida);
                }
                Err(e) => {
                    /*if e == RecvTimeoutError::Timeout {
                    }*/
                    if e == RecvTimeoutError::Disconnected {
                        println!("Cliente: No hay más PublishMessage's por leer.");
                        break;
                    }
                }
            }
        }
    }

    // Cliente termina de utilizar mqtt
    if let Ok(mut mqtt_client) = mqtt_client.lock() {
        mqtt_client.finalizar();
    }
}

fn publish_incident(
    sistema_monitoreo: &Arc<Mutex<SistemaMonitoreo>>,
    mqtt_client: &Arc<Mutex<MQTTClient>>,
) {
    if let Ok(mut sistema_monitoreo_lock) = sistema_monitoreo.lock() {
        if let Ok(mut incidents) = sistema_monitoreo_lock.get_incidents().lock() {
            if !incidents.is_empty() {
                for incident in incidents.iter_mut() {
                    if !incident.sent {
                        println!("Sistema-Monitoreo: Publicando incidente.");

                        // Hago el publish
                        if let Ok(mut mqtt_client) = mqtt_client.lock() {
                            let res = mqtt_client.mqtt_publish("Inc", &incident.to_bytes());
                            match res {
                                Ok(_) => {
                                    println!("Sistema-Monitoreo: Ha hecho un publish");

                                    //sistema_monitoreo_lock.mark_incident_as_sent(incident.id);
                                    incident.sent = true;
                                }
                                Err(e) => {
                                    println!("Sistema-Monitoreo: Error al hacer el publish {:?}", e)
                                }
                            };
                        }
                    }
                }
            };

            //sistema_monitoreo_lock.get_incidents();
        }
    }
}

fn main() {
    //Recibe por consola la dirección IP y el puerto del servidor
    let res = load_ip_and_port();
    let (ip, port) = match res {
        Ok((ip, port)) => (ip, port),
        Err(e) => {
            println!("Error al cargar el puerto: {:?}", e);
            return;
        }
    };

    let broker_addr: String = format!("{}:{}", ip, port);
    let broker_addr = broker_addr
        .parse::<SocketAddr>()
        .expect("Dirección no válida");

    let sistema_monitoreo = Arc::new(Mutex::new(SistemaMonitoreo::new())); // Create a new instance of `SistemaMonitoreo` and wrap it in an `Arc<Mutex<_>>`
    let sistema_monitoreo_ui = Arc::clone(&sistema_monitoreo); // Clone the `Arc` for the UI thread
    
    // Hijos para publish y subscribe
    let mqtt_client_res = establish_mqtt_broker_connection(&broker_addr);

    let mut hijos: Vec<JoinHandle<()>> = vec![];
    match mqtt_client_res {
        Ok(mqtt_client) => {
            let mqtt_client_sh = Arc::new(Mutex::new(mqtt_client));

            // Hijo para subscribe cameras
            let mqtt_client_sh_clone = Arc::clone(&mqtt_client_sh);

            let hijo_connect = thread::spawn(move || {
                subscribe_to_topics(mqtt_client_sh_clone);
            });
            hijos.push(hijo_connect);

            // Hijo para publish incidents
            let mqtt_client_incident_sh_clone = Arc::clone(&mqtt_client_sh);

            let hijo_send_incidents = thread::spawn(move || loop {
                publish_incident(&sistema_monitoreo, &mqtt_client_incident_sh_clone);

                // Esperamos, para publicar los cambios "periódicamente"
                thread::sleep(std::time::Duration::from_secs(5));
            });

            hijos.push(hijo_send_incidents);
        }

        Err(e) => println!(
            "Error al establecer la conexión con el broker MQTT: {:?}",
            e
        ),
    }

    // Hijo para ui
    let hijo_ui = thread::spawn(move || {
        let application = Rc::new(RefCell::new(
            gtk::Application::new(
                Some("fi.uba.sistemamonitoreo"),
                gio::ApplicationFlags::FLAGS_NONE,
            )
            .expect("Fallo en iniciar la aplicacion"),
        ));

        let application_clone = Rc::clone(&application);

        application.borrow_mut().connect_activate(move |_| {
            build_ui(&application_clone.borrow(), &sistema_monitoreo_ui)
        });

        application.borrow().run(&[]);
    });

    hijos.push(hijo_ui);

    // Espera a que todos los hilos terminen.
    for hijo in hijos {
        if let Err(e) = hijo.join() {
            eprintln!("Error al esperar el hilo: {:?}", e);
        }
    }
}

fn build_ui(application: &gtk::Application, sistema_monitoreo: &Arc<Mutex<SistemaMonitoreo>>) {
    let window = gtk::ApplicationWindow::new(application);
    window.set_title("Sistema de Monitoreo");
    window.set_default_size(800, 600);

    let menubar = gtk::MenuBar::new();

    let menu_incidents = gtk::Menu::new();
    let item_add = gtk::MenuItem::with_label("Alta de Incidente");
    let item_edit = gtk::MenuItem::with_label("Modificación de Incidente");
    let item_delete = gtk::MenuItem::with_label("Baja de Incidente");

    menu_incidents.append(&item_add);
    menu_incidents.append(&item_edit);
    menu_incidents.append(&item_delete);

    let sistema_monitoreo_clone = Arc::clone(sistema_monitoreo);
    item_add.connect_activate(move |_| {
        show_add_form(&sistema_monitoreo_clone);
    });

    item_edit.connect_activate(|_| {
        println!("Modificación de Incidente");
    });

    item_delete.connect_activate(|_| {
        println!("Baja de Incidente");
    });

    let item_quit = gtk::MenuItem::with_label("Salir");
    item_quit.connect_activate(|_| {
        gtk::main_quit();
    });

    let menu_app = gtk::Menu::new();
    menu_app.append(&item_quit);

    let item_incidents = gtk::MenuItem::with_label("Incidentes");
    let item_exit = gtk::MenuItem::with_label("Salir");

    item_incidents.set_submenu(Some(&menu_incidents));
    item_exit.connect_activate(|_| {
        gtk::main_quit();
    });

    menubar.append(&item_incidents);
    menubar.append(&item_exit);

    // Un layout para el menubar y la estructura de imágenes superpuestas
    let layout = gtk::Box::new(gtk::Orientation::Vertical, 0);
    layout.pack_start(&menubar, false, false, 0);

    // Se crea un overlay para permitir superposición de imágenes
    let overlay = gtk::Overlay::new();
    layout.pack_start(&overlay, true, true, 0);

    // Imagen base
    let map_image = gtk::Image::from_file("mapa_temp.png");
    overlay.add(&map_image);

    // Imagen superpuesta: dron
    let superimposed_image = gtk::Image::from_file("dron_70.png");
    overlay.add_overlay(&superimposed_image);
    gtk::WidgetExt::set_halign(&superimposed_image, gtk::Align::Start);
    gtk::WidgetExt::set_valign(&superimposed_image, gtk::Align::Start);
    gtk::WidgetExt::set_margin_start(&superimposed_image, 100);
    gtk::WidgetExt::set_margin_top(&superimposed_image, 100);
    overlay.set_child_index(&superimposed_image, 0);

    let dron2 = gtk::Image::from_file("dron_70.png");
    overlay.add_overlay(&dron2.clone());
    gtk::WidgetExt::set_halign(&dron2, gtk::Align::Start);
    gtk::WidgetExt::set_valign(&dron2, gtk::Align::Start);
    gtk::WidgetExt::set_margin_start(&dron2, 200);
    gtk::WidgetExt::set_margin_top(&dron2, 300);
    overlay.set_child_index(&dron2, 0);

    // Imagen superpuesta: cámara
    let cam_img = gtk::Image::from_file("camara_50.png");
    overlay.add_overlay(&cam_img);
    gtk::WidgetExt::set_halign(&cam_img, gtk::Align::Start);
    gtk::WidgetExt::set_valign(&cam_img, gtk::Align::Start);
    gtk::WidgetExt::set_margin_start(&cam_img, 600);
    gtk::WidgetExt::set_margin_top(&cam_img, 400);
    overlay.set_child_index(&cam_img, 0);

    window.add(&layout);

    window.show_all();
}

fn show_add_form(sistema_monitoreo: &Arc<Mutex<SistemaMonitoreo>>) {
    let dialog = gtk::Dialog::with_buttons(
        Some("Alta de Incidente"),
        None::<&gtk::Window>,
        gtk::DialogFlags::MODAL,
        &[
            ("Ok", gtk::ResponseType::Ok),
            ("Cancel", gtk::ResponseType::Cancel),
        ],
    );

    let content_area = dialog.get_content_area();
    content_area.set_spacing(5);

    // let name_label = gtk::Label::new(Some("Nombre del Incidente:"));
    // let name_entry = gtk::Entry::new();
    // content_area.add(&name_label);
    // content_area.add(&name_entry);

    let x_label = gtk::Label::new(Some("Coordenada X:"));
    let x_entry = gtk::Entry::new();
    content_area.add(&x_label);
    content_area.add(&x_entry);

    let y_label = gtk::Label::new(Some("Coordenada Y:"));
    let y_entry = gtk::Entry::new();
    content_area.add(&y_label);
    content_area.add(&y_entry);

    dialog.show_all();

    let sistema_monitoreo_clone = Arc::clone(sistema_monitoreo);

    dialog.connect_response(move |dialog, response_type| {
        if response_type == gtk::ResponseType::Ok {
            let x_coord = x_entry.get_text().to_string();
            let y_coord = y_entry.get_text().to_string();

            println!("Coordenada X: {}", x_coord);
            println!("Coordenada Y: {}", y_coord);

            let x = x_coord.parse::<u8>().unwrap();
            let y = y_coord.parse::<u8>().unwrap();

            if let Ok(mut sistema_monitoreo_lock) = sistema_monitoreo_clone.lock() {
                let incident =
                    Incident::new(sistema_monitoreo_lock.generate_new_incident_id(), x, y);
                sistema_monitoreo_lock.add_incident(incident);
                println!(
                    "Incidente agregado: {:?}",
                    sistema_monitoreo_lock.get_incidents()
                );
            }
        }

        dialog.close();
    });
}

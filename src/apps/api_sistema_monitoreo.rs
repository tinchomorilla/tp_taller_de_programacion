use std::{
    error::Error,
    net::SocketAddr,
    sync::{mpsc, Arc, Mutex},
    thread::{self, JoinHandle},
};

use std::sync::mpsc::Receiver;

use crossbeam::channel::{self, Sender};

use crate::{messages::publish_message::PublishMessage, mqtt_client::MQTTClient};

use super::{incident::Incident, ui_sistema_monitoreo::UISistemaMonitoreo};

#[derive(Debug)]
pub struct SistemaMonitoreo {
    pub incidents: Arc<Mutex<Vec<Incident>>>,
    pub publish_message_tx: Sender<PublishMessage>,
}

impl SistemaMonitoreo {
    pub fn new() -> Self {
        // Crear un canal que acepte mensajes de tipo PublishMessage
        let (publish_message_tx, publish_message_rx) = channel::unbounded::<PublishMessage>();
        let (tx, rx) = mpsc::channel::<Incident>();

        let mut children: Vec<JoinHandle<()>> = vec![];
        let broker_addr = get_broker_address();

        let sistema_monitoreo = Self {
            incidents: Arc::new(Mutex::new(Vec::new())),
            publish_message_tx,
        };

        match establish_mqtt_broker_connection(&broker_addr) {
            Ok(mqtt_client) => {
                let mqtt_client_sh = Arc::new(Mutex::new(mqtt_client));
                let mqtt_client_sh_clone: Arc<Mutex<MQTTClient>> = Arc::clone(&mqtt_client_sh);

                let send_subscribe_thread =
                    sistema_monitoreo.spawn_subscribe_to_topics_thread(mqtt_client_sh);
                children.push(send_subscribe_thread);

                let mqtt_client_incident_sh_clone = Arc::clone(&mqtt_client_sh_clone);

                let send_incidents_thread = sistema_monitoreo
                    .spawn_send_incidents_thread(mqtt_client_incident_sh_clone, rx);
                children.push(send_incidents_thread);
            }
            Err(e) => println!(
                "Error al establecer la conexión con el broker MQTT: {:?}",
                e
            ),
        }

        let tx_clone = tx.clone();

        let _ = eframe::run_native(
            "Sistema Monitoreo",
            Default::default(),
            Box::new(|cc| {
                Box::new(UISistemaMonitoreo::new(
                    cc.egui_ctx.clone(),
                    tx_clone,
                    publish_message_rx,
                ))
            }),
        );

        join_all_threads(children);

        sistema_monitoreo
    }

    pub fn spawn_send_incidents_thread(
        &self,
        mqtt_client: Arc<Mutex<MQTTClient>>,
        rx: Receiver<Incident>,
    ) -> JoinHandle<()> {
        thread::spawn(move || loop {
            while let Ok(msg) = rx.recv() {
                publish_incident(msg, &mqtt_client);
            }
        })
    }

    // pub fn clone_ref(&self) -> Self {
    //     Self {
    //         incidents: self.incidents.clone(),
    //         camera_tx: self.camera_tx.clone(),
    //         dron_tx: self.dron_tx.clone(),
    //     }
    // }

    pub fn clone_ref(&self) -> Self {
        Self {
            incidents: self.incidents.clone(),
            publish_message_tx: self.publish_message_tx.clone(),
        }
    }

    pub fn spawn_subscribe_to_topics_thread(
        &self,
        mqtt_client: Arc<Mutex<MQTTClient>>,
    ) -> JoinHandle<()> {
        let self_clone = self.clone_ref();
        thread::spawn(move || {
            self_clone.subscribe_to_topics(mqtt_client);
        })
    }

    pub fn finalize_mqtt_client(&self, mqtt_client: &Arc<Mutex<MQTTClient>>) {
        if let Ok(mut mqtt_client) = mqtt_client.lock() {
            mqtt_client.finalizar();
        }
    }

    pub fn publish_incident(&self, incident: Incident, mqtt_client: &Arc<Mutex<MQTTClient>>) {
        println!("Sistema-Monitoreo: Publicando incidente.");

        // Hago el publish
        if let Ok(mut mqtt_client) = mqtt_client.lock() {
            let res = mqtt_client.mqtt_publish("Inc", &incident.to_bytes());
            match res {
                Ok(_) => {
                    println!("Sistema-Monitoreo: Ha hecho un publish");
                }
                Err(e) => {
                    println!("Sistema-Monitoreo: Error al hacer el publish {:?}", e)
                }
            };
        }
    }

    pub fn subscribe_to_topics(&self, mqtt_client: Arc<Mutex<MQTTClient>>) {
        self.subscribe_to_topic(&mqtt_client, "Cam");
        self.subscribe_to_topic(&mqtt_client, "Dron");
        self.receive_messages_from_subscribed_topics(&mqtt_client);
        finalize_mqtt_client(&mqtt_client);
    }

    pub fn subscribe_to_topic(&self, mqtt_client: &Arc<Mutex<MQTTClient>>, topic: &str) {
        if let Ok(mut mqtt_client) = mqtt_client.lock() {
            let res_sub = mqtt_client.mqtt_subscribe(vec![(String::from(topic))]);
            match res_sub {
                Ok(_) => println!("Cliente: Hecho un subscribe a topic {}", topic),
                Err(e) => println!("Cliente: Error al hacer un subscribe a topic: {:?}", e),
            }
        }
    }

    // Recibe mensajes de los topics a los que se ha suscrito
    pub fn receive_messages_from_subscribed_topics(&self, mqtt_client: &Arc<Mutex<MQTTClient>>) {
        loop {
            if let Ok(mqtt_client) = mqtt_client.lock() {
                match mqtt_client.mqtt_receive_msg_from_subs_topic() {
                    //Publish message: camera o dron
                    Ok(msg) => self.send_publish_message_to_ui(msg),
                    Err(e) => {
                        if !handle_message_receiving_error(e) {
                            break;
                        }
                    }
                }
            }
        }
    }

    pub fn send_publish_message_to_ui(&self, msg: PublishMessage) {
        let res_send = self.publish_message_tx.send(msg);
        match res_send {
            Ok(_) => println!("Cliente: Enviado mensaje a la UI"),
            Err(e) => println!("Cliente: Error al enviar mensaje a la UI: {:?}", e),
        }
    }

    pub fn add_incident(&mut self, incident: Incident) {
        self.incidents.lock().unwrap().push(incident);
    }

    pub fn get_incidents(&mut self) -> Arc<Mutex<Vec<Incident>>> {
        self.incidents.clone()
    }

    pub fn generate_new_incident_id(&self) -> u8 {
        let mut new_inc_id: u8 = 0;
        if let Ok(incidents) = self.incidents.lock() {
            new_inc_id = (incidents.len() + 1) as u8;
        }
        new_inc_id
    }
}

fn get_broker_address() -> SocketAddr {
    let (ip, port) = load_ip_and_port().unwrap_or_else(|e| {
        println!("Error al cargar el puerto: {:?}", e);
        std::process::exit(1);
    });

    let broker_addr: String = format!("{}:{}", ip, port);
    broker_addr.parse().expect("Dirección no válida")
}

fn join_all_threads(children: Vec<JoinHandle<()>>) {
    for hijo in children {
        if let Err(e) = hijo.join() {
            eprintln!("Error al esperar el hilo: {:?}", e);
        }
    }
}

/// Lee el IP del cliente y el puerto en el que el cliente se va a conectar al servidor.
pub fn load_ip_and_port() -> Result<(String, u16), Box<dyn Error>> {
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

pub fn establish_mqtt_broker_connection(
    broker_addr: &SocketAddr,
) -> Result<MQTTClient, Box<dyn std::error::Error>> {
    let client_id = "Sistema-Monitoreo";
    let mqtt_client_res = MQTTClient::mqtt_connect_to_broker(client_id, broker_addr);
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

pub fn handle_message_receiving_error(e: std::io::Error) -> bool {
    match e.kind() {
        std::io::ErrorKind::TimedOut => true,
        std::io::ErrorKind::NotConnected => {
            println!("Cliente: No hay más PublishMessage's por leer.");
            false
        }
        _ => {
            println!("Cliente: error al leer los publish messages recibidos.");
            true
        }
    }
    /*/*if e == RecvTimeoutError::Timeout {
    }*/

    if e == RecvTimeoutError::Disconnected {
        println!("Cliente: No hay más PublishMessage's por leer.");
        break;
    }*/
}

pub fn finalize_mqtt_client(mqtt_client: &Arc<Mutex<MQTTClient>>) {
    if let Ok(mut mqtt_client) = mqtt_client.lock() {
        mqtt_client.finalizar();
    }
}

pub fn publish_incident(incident: Incident, mqtt_client: &Arc<Mutex<MQTTClient>>) {
    println!("Sistema-Monitoreo: Publicando incidente.");

    // Hago el publish
    if let Ok(mut mqtt_client) = mqtt_client.lock() {
        let res = mqtt_client.mqtt_publish("Inc", &incident.to_bytes());
        match res {
            Ok(_) => {
                println!("Sistema-Monitoreo: Ha hecho un publish");
            }
            Err(e) => {
                println!("Sistema-Monitoreo: Error al hacer el publish {:?}", e)
            }
        };
    }
}

impl Default for SistemaMonitoreo {
    fn default() -> Self {
        Self::new()
    }
}

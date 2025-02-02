use std::{
    io::{self, ErrorKind},
    sync::{mpsc, Arc, Mutex},
    thread::{self, JoinHandle},
};

use crate::mqtt::{client::mqtt_client::MQTTClient, messages::publish_message::PublishMessage};
use crossbeam_channel::{unbounded, Receiver as CrossbeamReceiver, Sender as CrossbeamSender};
use std::sync::mpsc::{Receiver as MpscReceiver, Sender as MpscSender};

use crate::{
    apps::{
        apps_mqtt_topics::AppsMqttTopics,
        common_clients::{exit_when_asked, there_are_no_more_publish_msgs},
        incident_data::incident::Incident,
        sist_monitoreo::{order_checker::OrderChecker, ui_sistema_monitoreo::UISistemaMonitoreo},
    },
    logging::string_logger::StringLogger,
};

use std::fs;
use std::io::Error;

/// Sistema encargado de permitir la publicación de incidentes, determinar su estado; recibir información
/// sobre Cámaras, Drones, e Incidentes creados por el Sistema Cámaras, y mostrarla en una interfaz gráfica.
#[derive(Debug)]
pub struct SistemaMonitoreo {
    incidents: Arc<Mutex<Vec<Incident>>>,
    qos: u8,
    logger: StringLogger,
    topics: Vec<(String, u8)>,
}

fn leer_qos_desde_archivo(ruta_archivo: &str) -> Result<u8, io::Error> {
    let contenido = fs::read_to_string(ruta_archivo)?;
    let inicio = contenido.find("qos=").ok_or(io::Error::new(
        ErrorKind::NotFound,
        "No se encontró la etiqueta 'qos='",
    ))?;

    let valor_qos = contenido[inicio + 4..].trim().parse::<u8>().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "El valor de QoS no es un número válido",
        )
    })?;
    println!("Valor de QoS: {}", valor_qos);
    Ok(valor_qos)
}

impl SistemaMonitoreo {
    /// Crea un Sistema Monitoreo.
    pub fn new(logger: StringLogger) -> Self {
        let qos =
            leer_qos_desde_archivo("src/apps/sist_monitoreo/qos_sistema_monitoreo.properties")
                .unwrap_or(0);
        println!("valor de QoS: {}", qos);
        let topics = vec![
            (AppsMqttTopics::CameraTopic.to_str().to_string(), qos),
            (AppsMqttTopics::DronTopic.to_str().to_string(), qos),
            (AppsMqttTopics::IncidentTopic.to_str().to_string(), qos),
            (AppsMqttTopics::DescTopic.to_str().to_string(), qos),
        ];
        let sistema_monitoreo: SistemaMonitoreo = Self {
            incidents: Arc::new(Mutex::new(Vec::new())), // []
            qos,
            logger,
            topics,
        };

        sistema_monitoreo
    }

    /// Lanza las partes internas del sistema monitoreo y las inicializa.
    pub fn spawn_threads(
        &self,
        publish_message_rx: MpscReceiver<PublishMessage>,
        mqtt_client: MQTTClient,
    ) -> Vec<JoinHandle<()>> {
        let (incident_tx, incident_rx) = mpsc::channel::<Incident>();
        let (exit_tx, exit_rx) = mpsc::channel::<bool>();

        let mut children: Vec<JoinHandle<()>> = vec![];
        let mqtt_client_sh = Arc::new(Mutex::new(mqtt_client));
        let (egui_tx, egui_rx) = unbounded::<PublishMessage>();

        // Exit, cuando ui lo solicite
        children.push(self.spawn_exit_thread(mqtt_client_sh.clone(), exit_rx));

        // Recibe inc de la ui y hace publish
        children.push(self.spawn_publish_incs_thread(mqtt_client_sh.clone(), incident_rx));

        // Recibe msgs por MQTT y los envía para mostrarse en la ui
        children.push(self.spawn_subscribe_to_topics_thread(
            mqtt_client_sh.clone(),
            publish_message_rx,
            egui_tx,
        ));

        // UI
        self.spawn_ui_thread(incident_tx, egui_rx, exit_tx);

        children
    }
    pub fn get_qos(&self) -> u8 {
        self.qos
    }

    /// Hilo encargado de lanzar la UI.
    fn spawn_ui_thread(
        &self,
        incident_tx: MpscSender<Incident>,
        publish_message_rx: CrossbeamReceiver<PublishMessage>,
        exit_tx: MpscSender<bool>,
    ) {
        if let Err(e) = eframe::run_native(
            "Sistema Monitoreo",
            Default::default(),
            Box::new(|cc| {
                Box::new(UISistemaMonitoreo::new(
                    cc.egui_ctx.clone(),
                    incident_tx,
                    publish_message_rx,
                    exit_tx,
                ))
            }),
        ) {
            self.logger.log(format!("Error en hilo para UI: {:?}.", e));
        }
        println!("Saliendo de ui.");
    }

    /// Recibe incidente desde la UI, y lo publica por MQTT.
    fn spawn_publish_incs_thread(
        &self,
        mqtt_client: Arc<Mutex<MQTTClient>>,
        rx: MpscReceiver<Incident>,
    ) -> JoinHandle<()> {
        let self_clone = self.clone_ref();
        thread::spawn(move || {
            while let Ok(inc) = rx.recv() {
                self_clone
                    .logger
                    .log(format!("Sistema-Monitoreo: envío incidente: {:?}", inc));
                self_clone.publish_incident(inc, &mqtt_client);
            }
        })
    }

    fn clone_ref(&self) -> Self {
        Self {
            incidents: self.incidents.clone(),
            qos: self.qos,
            logger: self.logger.clone_ref(),
            topics: self.topics.clone(),
        }
    }

    /// Se suscribe a los topics y queda recibiendo PublishMessages de esos topics.
    /// Delega el procesamiento de cada mensaje recibido por MQTT a otra parte del Sistema Cámaras, enviándolo por un channel.
    fn spawn_subscribe_to_topics_thread(
        &self,
        mqtt_client: Arc<Mutex<MQTTClient>>,
        mqtt_rx: MpscReceiver<PublishMessage>,
        egui_tx: CrossbeamSender<PublishMessage>,
    ) -> JoinHandle<()> {
        let mut self_clone = self.clone_ref();
        thread::spawn(move || {
            if let Err(e) = self_clone.subscribe_and_receive_msgs(&mqtt_client, mqtt_rx, egui_tx) {
                self_clone.logger.log(format!(
                    "Error en hilo para suscribir y recibir mensajes de MQTT: {:?}.",
                    e
                ));
            }
        })
    }

    /// Se suscribe a los topics de interés y permanece escuchando mensajes recibidos de los mismos.
    fn subscribe_and_receive_msgs(
        &mut self,
        mqtt_client: &Arc<Mutex<MQTTClient>>,
        mqtt_rx: MpscReceiver<PublishMessage>,
        egui_tx: CrossbeamSender<PublishMessage>,
    ) -> Result<(), Error> {
        self.subscribe_to_topics(mqtt_client)?;
        self.logger.log(format!("Suscripto a {:?}", &self.topics));
        self.receive_messages_from_subscribed_topics(mqtt_rx, egui_tx);
        Ok(())
    }

    /// Utiliza la librería MQTT para subscribirse a los topics.
    fn subscribe_to_topics(&self, mqtt_client: &Arc<Mutex<MQTTClient>>) -> Result<(), Error> {
        if let Ok(mut mqtt_client) = mqtt_client.lock() {
            mqtt_client.mqtt_subscribe(self.topics.clone())?;
            Ok(())
        } else {
            Err(Error::new(
                ErrorKind::Other,
                "Error al obtener el lock del mqtt_client",
            ))
        }
    }

    /// Si el mensaje publish recibido por MQTT es más nuevo que el último procesado, entonces
    /// envía a otra parte del sistema de monitoreo, para ser procesado.
    fn receive_messages_from_subscribed_topics(
        &mut self,
        mqtt_rx: MpscReceiver<PublishMessage>,
        egui_tx: CrossbeamSender<PublishMessage>,
    ) {
        let mut time_order_checker = OrderChecker::new();

        for pub_msg in mqtt_rx {
            self.logger.log(format!("Publish recibido: {:?}", pub_msg));
            // Chequeo el timestamp del publish_msg, si es nuevo, lo mando a la ui
            // Uso un match, no quiero retornar si fue error xq cortaría el loop, solo lo loggueo
            match time_order_checker.is_newest(&pub_msg) {
                Ok(true) => self.send_publish_message_to_ui(pub_msg, egui_tx.clone()),
                Ok(false) => {}, // No se lo procesa porque no es el más nuevo
                Err(e) => self.logger.log(format!("Error en OrderChecker: {:?}", e)),                
            }
        }

        there_are_no_more_publish_msgs(&self.logger);
    }

    fn send_publish_message_to_ui(
        &self,
        msg: PublishMessage,
        egui_tx: CrossbeamSender<PublishMessage>,
    ) {
        let res_send = egui_tx.send(msg);
        match res_send {
            Ok(_) => println!("Enviado mensaje a la UI"),
            Err(e) => println!("Error al enviar mensaje a la UI: {:?}", e),
        }
    }

    /// Hilo para salir desde la UI
    fn spawn_exit_thread(
        &self,
        mqtt_client: Arc<Mutex<MQTTClient>>,
        exit_rx: MpscReceiver<bool>,
    ) -> JoinHandle<()> {
        thread::spawn(move || {
            exit_when_asked(mqtt_client, exit_rx);
        })
    }

    /// Utiliza la librería MQTT para publicar el `incident` al topic de incidentes.
    fn publish_incident(&self, incident: Incident, mqtt_client: &Arc<Mutex<MQTTClient>>) {
        println!("Publicando incidente...");
        self.logger.log("Publicando incidente...".to_string());

        // Hago el publish
        if let Ok(mut mqtt_client) = mqtt_client.lock() {
            let res_publish = mqtt_client.mqtt_publish(
                AppsMqttTopics::IncidentTopic.to_str(),
                &incident.to_bytes(),
                self.get_qos(),
            );
            match res_publish {
                Ok(publish_msg) => {
                    self.logger
                        .log(format!("Publish enviado:{:?}", publish_msg));
                }
                Err(e) => {
                    self.logger.log(format!("Error al enviar publish {:?}", e));
                }
            };
        }
    }
}

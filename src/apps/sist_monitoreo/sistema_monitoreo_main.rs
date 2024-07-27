use std::io::Error;
use std::{
    sync::mpsc,
    thread::{self},
};

use crossbeam_channel::unbounded;
use rustx::logging::string_logger::StringLogger;
use rustx::mqtt::mqtt_utils::will_message_utils::app_type::AppType;
use rustx::mqtt::mqtt_utils::will_message_utils::will_content::WillContent;
use rustx::{
    apps::{
        common_clients::{get_broker_address, join_all_threads},
        sist_monitoreo::sistema_monitoreo::SistemaMonitoreo,
    },
    mqtt::{
        client::{
            mqtt_client::MQTTClient, mqtt_client_listener::MQTTClientListener,
            mqtt_client_server_connection::mqtt_connect_to_broker,
        },
        messages::publish_message::PublishMessage,
    },
};

type Channels = (
    mpsc::Sender<PublishMessage>,
    mpsc::Receiver<PublishMessage>,
    crossbeam_channel::Sender<PublishMessage>,
    crossbeam_channel::Receiver<PublishMessage>,
);

fn create_channels() -> Channels {
    let (publish_message_tx, publish_message_rx) = mpsc::channel::<PublishMessage>();
    let (egui_tx, egui_rx) = unbounded::<PublishMessage>();
    (publish_message_tx, publish_message_rx, egui_tx, egui_rx)
}

fn get_formatted_app_id() -> String {
    String::from("Sistema-Monitoreo")
}

fn get_app_will_msg_content() -> WillContent {
    WillContent::new(AppType::Monitoreo, 0) // []
}

fn get_app_will_msg_content() -> WillContent {
    WillContent::new(AppType::Monitoreo, 0) // []
}

fn main() -> Result<(), Error> {
    let broker_addr = get_broker_address();

    // Los logger_tx y logger_rx de este tipo de datos, podrían eliminarse por ser reemplazados por el nuevo string logger; se conservan temporalmente por compatibilidad hacia atrás.
    let (publish_message_tx, publish_message_rx, egui_tx, egui_rx) = create_channels();

    // Se crean y configuran ambos extremos del string logger
    let (logger, handle_logger) = StringLogger::create_logger(get_formatted_app_id());

    let qos = 1; // []
    let client_id = get_formatted_app_id();
    let will_msg_content = get_app_will_msg_content();
    match mqtt_connect_to_broker(client_id.as_str(), &broker_addr, will_msg_content, rustx::apps::apps_mqtt_topics::AppsMqttTopics::DescTopic, qos){
        Ok(stream) => {
            let mut mqtt_client_listener =
                MQTTClientListener::new(stream.try_clone()?, publish_message_tx);
            let mqtt_client: MQTTClient = MQTTClient::new(stream, mqtt_client_listener.clone());
            let sistema_monitoreo = SistemaMonitoreo::new(egui_tx, logger);
            println!("Cliente: Conectado al broker MQTT.");
            let handler_1 = thread::spawn(move || {
                let _ = mqtt_client_listener.read_from_server();
            });

            let mut handlers =
                sistema_monitoreo.spawn_threads(publish_message_rx, egui_rx, mqtt_client);

            handlers.push(handler_1);
            join_all_threads(handlers);
        }
        Err(e) => println!(
            "Sistema-Monitoreo: Error al conectar al broker MQTT: {:?}",
            e
        ),
    }

    // Se espera al hijo para el logger writer
    if handle_logger.join().is_err() {
        println!("Error al esperar al hijo para string logger writer.")
    }

    Ok(())
}

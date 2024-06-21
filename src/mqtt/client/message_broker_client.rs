use std::thread;

use config::{Config, File, FileFormat};
use rustx::mqtt::client::mqtt_client::MQTTClient;

// Este archivo representa a un cliente cualquiera. Así usará cada cliente a la librería MQTT.
fn main() {
    println!("Leyendo Archivo de Configuración");
    let mut config = Config::default();
    config
        .merge(File::new(
            "message_broker_client_config.properties",
            FileFormat::Toml,
        ))
        .unwrap();

    let ip = config
        .get::<String>("ip")
        .unwrap_or_else(|_| "127.0.0.1".to_string());
    let port = config.get::<u16>("port").unwrap_or(9090);
    let broker_addr = format!("{}:{}", ip, port)
        .parse()
        .expect("Dirección no válida");
    // Cliente usa funciones connect, publish, y subscribe de la lib.
    let client_id = "test-client";
    let mqtt_client_res = MQTTClient::mqtt_connect_to_broker(client_id, &broker_addr);
    match mqtt_client_res {
        Ok(mut mqtt_client) => {
            println!("Cliente: Conectado al broker MQTT.");

            // Cliente usa subscribe
            let res_sub = mqtt_client.mqtt_subscribe(vec![(String::from("topic3"))]);
            match res_sub {
                Ok(_) => println!("Cliente: Hecho un subscribe"),
                Err(e) => println!("Cliente: Error al hacer un subscribe: {:?}", e),
            }

            // Cliente usa publish
            //(topic: &str, payload: &[u8]
            let res = mqtt_client.mqtt_publish("topic3", "hola mundo :)".as_bytes());
            match res {
                Ok(_) => println!("Cliente: Hecho un publish"),
                Err(e) => println!("Cliente: Error al hacer el publish {:?}", e),
            }

            // Que lea del topic al/os cual/es hizo subscribe, implementando [].
            let h = thread::spawn(move || {
                while let Ok(msg) = mqtt_client.mqtt_receive_msg_from_subs_topic() {
                    println!("Cliente: Recibo estos msg_bytes: {:?}", msg);
                }

                // Cliente termina de utilizar mqtt
                mqtt_client.finish();
            });

            if h.join().is_err() {
                println!("Cliente: error al esperar a hijo que recibe msjs");
            }
        }
        Err(e) => println!("Cliente: Error al conectar al broker MQTT: {:?}", e),
    }
}
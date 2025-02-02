use std::sync::mpsc::Receiver;

//use rayon::ThreadPool;

use rayon::ThreadPool;

use crate::mqtt::messages::{
        packet_type::PacketType, puback_message::PubAckMessage, publish_message::PublishMessage,
        subscribe_message::SubscribeMessage, subscribe_return_code::SubscribeReturnCode,
};

use std::io::Error;

use super::{
    mqtt_server::MQTTServer,
    packet::Packet,
};

#[derive(Debug)]
pub struct MessageProcessor {
    mqtt_server: MQTTServer,
}

// fn contains_dron(input: &str) -> bool {
//     input.to_lowercase().contains("dron")
// }

impl MessageProcessor {
    pub fn new(mqtt_server: MQTTServer) -> Self {
        MessageProcessor { mqtt_server }
    }

    pub fn handle_packets(&mut self, rx_1: Receiver<Packet>) -> Result<(), Error> {

        // Con threadpool sería:
        match create_thread_pool_with(20) {
            Ok(thread_pool) => {
                for packet in rx_1 {
                    let self_clone = self.clone_ref();
                    thread_pool.spawn(move || {
                        self_clone.process_packet(packet);
                    });
                }
            }
            Err(e) => {
                println!("   ERROR: {:?}", e);
                for packet in rx_1 {
                    self.process_packet(packet);
                }
            }
        }

        // Sin threadpool era:
        /*for packet in rx_1 {
            self.process_packet(packet); // Ejecuta en el hilo actual
        }*/     

        Ok(())
    }

    fn process_packet(&self, packet: Packet) {
        let msg_bytes = packet.get_msg_bytes();
        let client_id = packet.get_username();
        match packet.get_message_type() {
            PacketType::Publish => self.handle_publish(msg_bytes, client_id),
            PacketType::Subscribe => self.handle_subscribe(msg_bytes, client_id),
            PacketType::Puback => self.handle_puback(msg_bytes),
            _ => println!("   ERROR: Tipo de mensaje desconocido\n "),
        };
    }

    fn handle_publish(&self, msg_bytes: Vec<u8>, client_id: &str) {
        let publish_msg_res = PublishMessage::from_bytes(msg_bytes);
        match publish_msg_res {
            Ok(publish_msg) => {
                println!("Publish recibido, topic: {:?}, packet_id: {:?}", publish_msg.get_topic(), publish_msg.get_packet_id());
                let puback_res = self.send_puback_to(client_id, &publish_msg);
                if let Err(e) = puback_res {
                    println!("   Error en handle_publish: {:?}", e);
                }
                if let Err(e) = self.mqtt_server.handle_publish_message(&publish_msg){
                    // No quiero retornar si falló alguna operación hacia Un user, solamente logguearlo.
                    println!("   Error en handle_publish: {:?}", e);
                };                

            }
            Err(e) => println!("   Error en handle_publish: {:?}", e),
        }
    }

    fn handle_subscribe(&self, msg_bytes: Vec<u8>, client_id: &str) {
        let subscribe_msg_res = SubscribeMessage::from_bytes(msg_bytes);
        match subscribe_msg_res {
            Ok(msg) => {
                let return_codes_res = self.mqtt_server.add_topics_to_subscriber(client_id, &msg);
                let operation_result = self
                    .mqtt_server
                    .send_preexisting_msgs_to_new_subscriber(client_id, &msg);
                if let Err(e) = operation_result {
                    println!("   ERROR: {:?}", e);
                }
                let packet_id = msg.get_packet_id();
                let suback_res = self.send_suback_to(client_id, return_codes_res, packet_id);
                if let Err(e) = suback_res {
                    println!("   ERROR: {:?}", e);
                }
            }
            Err(e) => println!("   ERROR: {:?}", e),
        }
    }

    fn handle_puback(&self, msg_bytes: Vec<u8>) {
        let puback_msg_res = PubAckMessage::msg_from_bytes(msg_bytes);
        match puback_msg_res {
            Ok(puback_msg) => println!("Pub ack recibido, packet_id: {:?}", puback_msg.get_packet_id()),
            Err(e) => println!("   ERROR: {:?}", e),
        }
    }

    pub fn send_puback_to(
        &self,
        client_id: &str,
        publish_msg: &PublishMessage,
    ) -> Result<(), Error> {
        self.mqtt_server.send_puback_to(client_id, publish_msg)?;

        Ok(())
    }

    fn send_suback_to(
        &self,
        client_id: &str,
        return_codes_res: Result<Vec<SubscribeReturnCode>, Error>,
        packet_id: u16,
    ) -> Result<(), Error> {
        self.mqtt_server
            .send_suback_to(client_id, &return_codes_res, packet_id)?;
        Ok(())
    }
    // []
    fn clone_ref(&self) -> Self {
        MessageProcessor {
            mqtt_server: self.mqtt_server.clone_ref(),
        }
    }
}

fn create_thread_pool_with(num_threads: usize) -> Result<ThreadPool, Error> {
    match rayon::ThreadPoolBuilder::new()
        .num_threads(num_threads)
        .build()
    {
        Ok(thread_pool) => Ok(thread_pool),
        Err(e) => Err(Error::new(std::io::ErrorKind::Other, e)),
    }
}

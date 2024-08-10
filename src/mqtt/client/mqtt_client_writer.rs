use std::io::{Error, ErrorKind};

use crate::mqtt::mqtt_utils::utils::display_debug_publish_msg;
use crate::mqtt::stream_type::StreamType;
use crate::mqtt::{
    messages::{
        disconnect_message::DisconnectMessage, publish_flags::PublishFlags,
        publish_message::PublishMessage, subscribe_message::SubscribeMessage,
    },
    mqtt_utils::utils::write_message_to_stream,
};
use std::net::Shutdown;

use super::mqtt_client_retransmitter::MQTTClientRetransmitter;

#[derive(Debug)]
pub struct MQTTClientWriter {
    stream: StreamType,
    available_packet_id: u16,
    retransmitter: MQTTClientRetransmitter,
}

impl MQTTClientWriter {
    pub fn new(stream: StreamType) -> MQTTClientWriter {
        let (retransmitter, ack_tx) = MQTTClientRetransmitter::new();
        
        MQTTClientWriter {
            stream,
            available_packet_id: 0,
            retransmitter,
        }
    }

    pub fn mqtt_publish(
        &mut self,
        topic: &str,
        payload: &[u8],
        qos: u8,
    ) -> Result<PublishMessage, Error> {
        println!("-----------------");
        let packet_id = self.generate_packet_id();
        // Creo un msj publish
        let flags = PublishFlags::new(0, qos, 0)?;
        let result = PublishMessage::new(3, flags, topic, Some(packet_id), payload);
        let publish_message = match result {
            Ok(msg) => {
                //println!("Mqtt publish: envío publish: \n   {:?}", msg);
                display_debug_publish_msg(&msg);
                msg
            }
            Err(e) => return Err(Error::new(ErrorKind::Other, e)),
        };

        // Lo envío
        let bytes_msg = publish_message.to_bytes();
        write_message_to_stream(&bytes_msg, &mut self.stream)?;
        println!("Mqtt publish: envío bytes publish: \n   {:?}", bytes_msg);

        Ok(publish_message)
    }

    // Nuestras apps clientes llamarán a esta función (los drones, etc)
    /// Función parte de la interfaz para uso de clientes del protocolo MQTT.
    /// Recibe el packet id, y un vector de topics a los cuales cliente desea suscribirse.
    pub fn mqtt_subscribe(
        &mut self,
        topics_to_subscribe: Vec<(String, u8)>,
    ) -> Result<SubscribeMessage, Error> {
        let packet_id = self.generate_packet_id();
        println!("-----------------");
        // Construyo subscribe
        let subscribe_msg = SubscribeMessage::new(packet_id, topics_to_subscribe);
        println!("Mqtt subscribe: enviando mensaje: \n   {:?}", subscribe_msg);

        // Lo envío
        let subs_bytes = subscribe_msg.to_bytes();
        write_message_to_stream(&subs_bytes, &mut self.stream)?;
        println!(
            "Mqtt subscribe: enviado mensaje en bytes: \n   {:?}",
            subs_bytes
        );

        Ok(subscribe_msg)
    }

    /// Envía mensaje disconnect, y cierra la conexión con el servidor.
    pub fn mqtt_disconnect(&mut self) -> Result<(), Error> {
        let msg = DisconnectMessage::new();

        // Lo envío
        let bytes_msg = msg.to_bytes();
        write_message_to_stream(&bytes_msg, &mut self.stream)?;
        //println!("Mqtt disconnect: bytes {:?}", msg);

        // Cerramos la conexión con el servidor
        self.stream.shutdown(Shutdown::Both)?;

        Ok(())
    }

    /// Devuelve el packet_id a usar para el siguiente mensaje enviado.
    /// Incrementa en 1 el atributo correspondiente, debido a la llamada anterior, y devuelve el valor a ser usado
    /// en el envío para el cual fue llamada esta función.
    fn generate_packet_id(&mut self) -> u16 {
        self.available_packet_id += 1;
        self.available_packet_id
    }

    // Funciones relacionadas con el Retransmitter:

    /// Función para ser usada por el `Retransmitter`, cuando el mismo determinó que el `msg` debe enviarse por el stream
    /// a server.
    fn resend_msg(&mut self, msg: PublishMessage)-> Result<(), Error> {
        let bytes_msg = msg.to_bytes();
        write_message_to_stream(&bytes_msg, &mut self.stream)?;
        Ok(())
    }

    /// Al momento de recibir el ack lo habrá enviado / enviará por el ack_tx;
    /// pero además, desde que se llama a esta función se delega al Retransmitter la responsabilidad de
    /// ponerse a escuchar por el ack_rx lo que desde el ack_tx que mande el listener.
    pub fn wait_for_ack(&mut self, msg: PublishMessage) -> Result<(), Error> {
        // Si el Retransmitter determina que se debe volver a enviar el mensaje, lo envío.
        let option: Option<PublishMessage> = self.retransmitter.wait_for_ack(&msg)?;
        if let Some(_msg_to_resend) = option {
            self.resend_msg(msg)?
        }

        Ok(())
        
    }
}

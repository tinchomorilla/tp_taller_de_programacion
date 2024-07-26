use std::net::{SocketAddr, TcpStream};

use std::io::{self, Error, ErrorKind};

type StreamType = TcpStream;
use crate::apps::apps_mqtt_topics::AppsMqttTopics;
use crate::mqtt::messages::{connack_message::ConnackMessage,
                            connect_message::ConnectMessage,
                            connect_return_code::ConnectReturnCode,
                            packet_type::PacketType};
use crate::mqtt::mqtt_utils::utils::{
    get_fixed_header_from_stream_for_conn, get_whole_message_in_bytes_from_stream,
    write_message_to_stream,
};
use crate::mqtt::mqtt_utils::will_message_utils::will_content::WillContent;

pub struct MqttClientConnection {}

pub fn mqtt_connect_to_broker(client_id: &str, addr: &SocketAddr, will_msg_content: WillContent, will_topic: AppsMqttTopics, will_qos: u8) -> Result<TcpStream, Error> {
    //let will_topic = String::from("desc"); // PROBANDO
    //let will_qos = 1; // PROBANDO, ESTO VA POR PARÁMETRO
    // Inicializaciones
    // Intenta conectar al servidor MQTT
    let stream_tcp = TcpStream::connect(addr)
        .map_err(|_| io::Error::new(io::ErrorKind::Other, "error del servidor"))?;

    //let mut stream = Arc::new(Mutex::new(stream_tcp));
    let mut stream = stream_tcp;

    // Crea el mensaje tipo Connect y lo pasa a bytes
    let mut connect_msg = ConnectMessage::new(
        client_id.to_string(),
        Some(String::from(will_topic.to_str())),
        Some(String::from("dron-5")), // Quiero: will_msg_content.to_string()
        Some("usuario0".to_string()),
        Some("rustx123".to_string()),
        will_qos
    );

    // Intenta enviar el mensaje CONNECT al servidor MQTT
    let msg_bytes = connect_msg.to_bytes();
    write_message_to_stream(&msg_bytes, &mut stream)?;
    println!("Envía connect: \n   {:?}", &connect_msg);

    println!("Mqtt cliente leyendo: esperando connack.");
    // Leo un fixed header, deberá ser de un connect
    read_connack(&mut stream)?;

    Ok(stream)
}

/// Lee un fixed header y verifica que haya sido de tipo Connack
fn read_connack(stream: &mut StreamType) -> Result<(), Error> {
    // Lee un fixed header
    let (fixed_header_buf, fixed_header) = get_fixed_header_from_stream_for_conn(stream)?;

    let fixed_header_info = (fixed_header_buf, fixed_header);

    // Verifica que haya sido de tipo Connack
    let recvd_msg_type = fixed_header_info.1.get_message_type();
    if recvd_msg_type == PacketType::Connack {
        // ConnAck
        println!("Mqtt cliente leyendo: recibo conn ack");
        let recvd_bytes = get_whole_message_in_bytes_from_stream(
            &fixed_header_info.1,
            stream,
            &fixed_header_info.0,
        )?;
        // Entonces tengo el mensaje completo
        let msg = ConnackMessage::from_bytes(&recvd_bytes)?; //
        println!("   Mensaje conn ack completo recibido: {:?}", msg);
        let ret = msg.get_connect_return_code();
        if ret == ConnectReturnCode::ConnectionAccepted {
            Ok(())
        } else {
            Err(Error::new(ErrorKind::InvalidData, ""))
        }
    } else {
        // No debería darse
        Err(Error::new(ErrorKind::InvalidData, ""))
    }
}

use std::{io::Write, sync::mpsc::Receiver};

use crate::logging::structs_to_save_in_logger::{OperationType, StructsToSaveInLogger};

#[derive(Debug)]
pub struct Logger {
    pub logger_rx: Receiver<StructsToSaveInLogger>,
}

impl Logger {
    pub fn new(logger_rx: Receiver<StructsToSaveInLogger>) -> Self {
        Self { logger_rx }
    }

    pub fn write_in_file(&self, message: StructsToSaveInLogger) {
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("src/log.txt")
            .unwrap();

        match message {
            StructsToSaveInLogger::AppType(client_name, app_type, operation_type) => {
                let operation_str = match operation_type {
                    OperationType::Sent => "envió",
                    OperationType::Received => "recibió",
                };
                writeln!(
                    file,
                    "{} {} un mensaje de tipo: {:?}",
                    client_name, operation_str, app_type
                )
                .unwrap();
            }
            StructsToSaveInLogger::MessageType(client_name, message_type, operation_type) => {
                let operation_str = match operation_type {
                    OperationType::Sent => "envió",
                    OperationType::Received => "recibió",
                };
                writeln!(
                    file,
                    "{} {} un mensaje de tipo: {:?}",
                    client_name, operation_str, message_type
                )
                .unwrap();
            }
        }
    }
    // pub fn write_in_file(&self, message: StructsToSaveInLogger) {
    //     let mut file = std::fs::OpenOptions::new()
    //         .create(true)
    //         .append(true)
    //         .open("src/log.txt")
    //         .unwrap();

    //     match message {
    //         StructsToSaveInLogger::AppType(app_type) => {
    //             //println!("Mensaje de tipo AppType recibido: {:?}", app_type);
    //             writeln!(file, "{:?}", app_type).unwrap();
    //         }
    //         StructsToSaveInLogger::MessageType(message_type) => {
    //             println!("Mensaje de tipo MessageType recibido: {:?}", message_type);
    //             writeln!(file, "{:?}", message_type).unwrap();
    //         }
    //     }
    // }
}
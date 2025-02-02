#[derive(Debug)]
#[derive(PartialEq)]
pub struct ConnectFlags {
    pub username_flag: bool, // bit 7
    pub password_flag: bool, // bit 6
    pub will_retain: bool,   // bit 5
    pub will_qos: u8,        // bits 3-4
    pub will_flag: bool,     // bit 2
    pub clean_session: bool, // bit 1
    pub reserved: bool,      // bit 0
}

impl ConnectFlags {
    pub fn new(
        clean_session: bool,
        will_flag: bool,
        will_qos: u8,
        will_retain: bool,
        username_flag: bool,
        password_flag: bool,
        reserved: bool,
    ) -> Self {
        ConnectFlags {
            username_flag,
            clean_session,
            will_flag,
            will_qos,
            will_retain,
            password_flag,
            reserved,
        }
    }

    pub fn to_byte(&self) -> u8 {
        let mut byte = 0;
        if self.username_flag {
            byte |= 0x80;
        }
        if self.password_flag {
            byte |= 0x40;
        }
        if self.will_retain {
            byte |= 0x20;
        }
        byte |= (self.will_qos & 0x03) << 3;
        if self.will_flag {
            byte |= 0x04;
        }
        if self.clean_session {
            byte |= 0x02;
        }
        byte
    }

    pub fn from_byte(byte: u8) -> ConnectFlags {
        ConnectFlags {
            username_flag: (byte & 0x80) != 0,
            password_flag: (byte & 0x40) != 0,
            will_retain: (byte & 0x20) != 0,
            will_qos: (byte & 0x18) >> 3,
            will_flag: (byte & 0x04) != 0,
            clean_session: (byte & 0x02) != 0,
            reserved: (byte & 0x01) != 0,
        }
    }
}

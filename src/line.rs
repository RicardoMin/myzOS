use crate::console;

const LINE_CAPACITY: usize = 128;

pub struct LineEditor {
    buf: [u8; LINE_CAPACITY],
    len: usize,
    skip_lf: bool,
    discarding: bool,
}

impl LineEditor {
    pub fn new() -> Self {
        Self {
            buf: [0; LINE_CAPACITY],
            len: 0,
            skip_lf: false,
            discarding: false,
        }
    }

    pub fn feed(&mut self, byte: u8) -> Option<&str> {
        if self.skip_lf {
            self.skip_lf = false;

            if byte == b'\n' {
                return None;
            }
        }
        if byte == b'\r' || byte == b'\n' {
            self.skip_lf = byte == b'\r';
            let _ = console::print(format_args!("\r\n"));

            if self.discarding {
                self.discarding = false;
                self.len = 0;
                return None;
            }

            let completed_len = self.len;
            self.len = 0;

            return Some(
                core::str::from_utf8(&self.buf[..completed_len])
                    .expect("line editor stores printable ASCII only"),
            )
        }

        if self.discarding {
            return None;
        }
        match byte {
            b'\x08' | b'\x7F' => {
                if self.len > 0 {
                    self.len -= 1;
                    let _ = console::print(format_args!(
                        "\x08 \x08",
                    ));
                }
                None
            } 
            b' '..=b'~' => {
                if self.len == LINE_CAPACITY {
                    self.len = 0;
                    self.discarding = true;
                    let  _ = console::print(format_args!(
                        "\r\ninput: line too long; discard until Enter\r\n",
                    ));
                } else {
                    self.buf[self.len] = byte;
                    self.len += 1;
                    console::echo_byte(byte);
                }
                None
            }
            _ => None
        }
    }

    pub fn on_input_lost(&mut self, count: usize) {
        self.len = 0;
        self.discarding = true;
        self.skip_lf = false;
        let _ = console::print(format_args!(
            "\r\ninput: RX queue overflow ({} rejected bytes); discard until Enter\r\n",
            count,
        ));
    }
}
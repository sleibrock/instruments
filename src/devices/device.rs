// device.rs - a MIDI device abstraction

extern crate portmidi as pm;

/// The Device is an abstraction for generic MIDI read/write purposes.
/// You can implement any kind of Device abstraction using this as the
/// main source of I/O passthrough. Including it and some info about
/// the device enables you to create simple APIs for devices.
pub struct Device<'a> {
    pub input: pm::InputPort<'a>,
    pub output: pm::OutputPort<'a>,
}

impl Device<'_> {
    pub fn new<'a>(name: &'a str, ctx: &'a pm::PortMidi) -> Result<Device<'a>, String> {
        let mut output_id: Option<i32> = None;
        let mut input_id: Option<i32> = None;

        for dev in ctx.devices().expect("Failed to query devices") {
            println!("Device: {}, id: {}", dev.name(), dev.id());
            if dev.name() == name {
                if dev.is_output() {
                    output_id = Some(dev.id());
                }

                if dev.is_input() {
                    input_id = Some(dev.id());
                }
            }
        }

        match (output_id, input_id) {
            (Some(oid), Some(iid)) => {
                let out_port = ctx
                    .device(oid)
                    .expect("Failed to find matching output device");

                let in_port = ctx
                    .device(iid)
                    .expect("Failed to find matching input device");

                Ok(Device {
                    input: ctx
                        .input_port(in_port, 1024)
                        .expect("Failed to open input port"),
                    output: ctx
                        .output_port(out_port, 1024)
                        .expect("Failed to open output port"),
                })
            }
            _ => Err("Failed to create a device context".into()),
        }
    }

    pub fn write(&mut self, kind: u8, note: u8, vel: u8, extra: u8) -> bool {
        self.output.write_message([kind, note, vel, extra]).is_ok()
    }
}

// end device.rs
